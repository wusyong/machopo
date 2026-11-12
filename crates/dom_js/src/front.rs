//! V8's side of a member's call (`dom_api::Front`): what Blink's
//! generated callback does with its `FunctionCallbackInfo`. The arguments
//! are converted before the DOM's operation (WebIDL's `ToString` may run
//! script). The operation borrows the isolate's state from the call's
//! scope (`V8Front::read`, `V8Front::mutate`), and its side of the call
//! (`V8Op`) holds the scope as `bindings::NoGc`: `this` and the node
//! arguments are unwrapped once (V8's `Signature` checked the receiver's
//! interface), and a result that is the engine's already (a node's
//! wrapper, a string's cached V8 string, a primitive V8 needn't allocate)
//! is returned at once; another is kept: the DOM's string, or the node (a
//! root). V8 can't allocate in the operation, which holds no scope that
//! would let it: its allocation may GC, and V8's major GCs mark the DOM,
//! whose operation's nodes are no roots. After it, a kept result becomes
//! V8's (`finish`): the string made, the node's wrapper made and wrapped
//! in an operation of its own. A node the member makes (`[NewObject]`)
//! gets the wrapper made before the operation, wrapped in it: no root.
//!
//! The return value is written as soon as it is known, and only what is
//! left kept: a result written in parts, then read whole, can't forward
//! from its stores, which wait for the cache (a whole result kept and
//! read back made a `firstChild` 1.6 such conflicts).

use std::cell::Cell;
use std::mem::ManuallyDrop;

use dom::{
    DomString, EventTargetImpl, NameId, Node, NodeRef, NodeRoot, ObjectRef, ObjectRoot, TargetRef,
    Window, names,
};
use dom_api::typed::{BlobPartArg, Radius, Uint8ClampedArg};
use dom_api::{
    AnyArg, ApiCx, ApiError, Callback, ConstructorInfo, Front, FrontTypes, HtmlConstructed, OpFront,
};
use elise::{JsEdge, Persistent};

use crate::listeners::{self, ReportAt};
use crate::state::{self, Caches, Scope, State};
use crate::strings::{self, ArgString, StringCache};
use crate::{exception, templates};

/// A result kept in the operation, made V8's after it: what V8 must
/// allocate for. Boxed (`Call::kept`): a call with none drops a null
/// pointer, a check inlined; the enum's drop glue (its roots, its
/// sequence) runs only for one kept, off the getters' path.
pub(crate) enum Kept {
    /// Past a Smi: a heap number.
    U32(u32),
    F64(f64),
    String(DomString),
    /// A name of the table whose V8 string isn't made yet.
    Name(NameId, DomString),
    /// A node with no wrapper, and its interface.
    Node(NodeRoot, usize),
    /// An object with no wrapper, and its interface.
    Object(ObjectRoot, usize),
    /// An element being upgraded that has a wrapper: given, with the
    /// call's `new.target`'s prototype (`OpFront::construct_html`).
    Upgrading(NodeRoot),
    /// A custom element for `createElement` to construct with the
    /// definition's constructor, in the document (`OpFront::ret_construct`).
    Construct(Persistent<dom::CustomElementDefinition>, NodeRoot),
    /// An `ImageData`'s pixels, its array made of them (its buffer's
    /// bytes: no copy), then the call's result, if kept too
    /// (`OpFront::set_image_data_pixels`).
    Pixels(Persistent<dom::ImageData>, Vec<u8>, Option<Box<Kept>>),
}

/// What a call's arguments, `this` and `new.target` are read from: a
/// function's callback's (`FunctionCallbackArguments`), or an indexed
/// getter's (`IndexArgs`). The front end is generic over it, so a
/// function's call reads its arguments as before, with no branch.
pub(crate) trait CallArgs<'s> {
    /// `this` as V8 checked it, an object of one of the isolate's templates
    /// (`bindings::Receiver`), when the call's function has a signature or
    /// is an interceptor's: `bindings::unwrap_receiver` reads it.
    fn receiver(&self) -> Option<bindings::Receiver<'s>>;

    fn length(&self) -> i32;
    fn get(&self, index: i32) -> v8::Local<'s, v8::Value>;
    fn this(&self) -> v8::Local<'s, v8::Object>;
    fn new_target(&self) -> v8::Local<'s, v8::Value>;
}

/// A function's callback's arguments with no word of its receiver: a
/// constructor's, a static member's, a legacy caller's (`Checked` is a
/// call's whose receiver V8 checked).
impl<'s> CallArgs<'s> for v8::FunctionCallbackArguments<'s> {
    #[inline]
    fn receiver(&self) -> Option<bindings::Receiver<'s>> {
        None
    }

    #[inline]
    fn length(&self) -> i32 {
        v8::FunctionCallbackArguments::length(self)
    }

    #[inline]
    fn get(&self, index: i32) -> v8::Local<'s, v8::Value> {
        v8::FunctionCallbackArguments::get(self, index)
    }

    #[inline]
    fn this(&self) -> v8::Local<'s, v8::Object> {
        v8::FunctionCallbackArguments::this(self)
    }

    #[inline]
    fn new_target(&self) -> v8::Local<'s, v8::Value> {
        v8::FunctionCallbackArguments::new_target(self)
    }
}

/// A function's callback's arguments and its receiver, which V8 checked
/// against the function's signature (`bindings::StateMethod`).
pub(crate) struct Checked<'s> {
    pub(crate) args: v8::FunctionCallbackArguments<'s>,
    pub(crate) this: bindings::Receiver<'s>,
}

impl<'s> CallArgs<'s> for Checked<'s> {
    #[inline]
    fn receiver(&self) -> Option<bindings::Receiver<'s>> {
        Some(self.this)
    }

    #[inline]
    fn length(&self) -> i32 {
        self.args.length()
    }

    #[inline]
    fn get(&self, index: i32) -> v8::Local<'s, v8::Value> {
        self.args.get(index)
    }

    #[inline]
    fn this(&self) -> v8::Local<'s, v8::Object> {
        self.this.object()
    }

    #[inline]
    fn new_target(&self) -> v8::Local<'s, v8::Value> {
        self.args.new_target()
    }
}

/// An indexed getter's call (`object[i]`): its one argument the index, its
/// `this` the object (the interceptor's holder, or an iterator's target).
pub(crate) struct IndexArgs<'s> {
    pub(crate) index: v8::Local<'s, v8::Value>,
    pub(crate) this: v8::Local<'s, v8::Object>,
    /// The holder, as V8 gave it (an interceptor's call); none for an
    /// iterator's target, which is unwrapped whole.
    pub(crate) receiver: Option<bindings::Receiver<'s>>,
    pub(crate) undefined: v8::Local<'s, v8::Value>,
}

impl<'s> CallArgs<'s> for IndexArgs<'s> {
    #[inline]
    fn receiver(&self) -> Option<bindings::Receiver<'s>> {
        self.receiver
    }

    fn length(&self) -> i32 {
        1
    }

    fn get(&self, index: i32) -> v8::Local<'s, v8::Value> {
        if index == 0 {
            self.index
        } else {
            self.undefined
        }
    }

    fn this(&self) -> v8::Local<'s, v8::Object> {
        self.this
    }

    fn new_target(&self) -> v8::Local<'s, v8::Value> {
        self.undefined
    }
}

/// A member's call out of its operation: the scope (whose isolate's state
/// is the DOM's), the arguments, and what the call holds across its
/// operation (`Call`). `'r` is the result's (a property callback's
/// `ReturnValue` has a lifetime of its own).
pub(crate) struct V8Front<'a, 's, 'i, 'r, A: CallArgs<'s> = v8::FunctionCallbackArguments<'s>> {
    scope: Scope<'a, 's, 'i>,
    args: &'a A,
    call: Call<'s, 'r>,
}

/// What a call takes before its operation and leaves for after: lent to
/// the operation's side whole (one pointer, not each field copied in).
pub(crate) struct Call<'s, 'r> {
    /// The result: in the operation only as `OpReturn` (`full` needs the
    /// isolate, which a holder of the state hasn't).
    rv: bindings::ReturnSlot<'r>,
    /// Dropped by `Call`'s own `Drop` (as `rare`), out of line only if set.
    kept: ManuallyDrop<Option<Box<Kept>>>,
    /// A `[NewObject]`'s wrapper, made before the operation, and its
    /// interface.
    prepared: Option<(v8::Local<'s, v8::Object>, usize)>,
    /// The current realm's global, found before the operation of a
    /// constructor that may ask for its window (`prepare_window`).
    window: Option<v8::Local<'s, v8::Object>>,
    /// The callback argument taken (`Callback(0)`: no member takes two),
    /// a word every call clears, not a `Vec` it makes and drops: an
    /// object (a listener's callback, a timer's function) or a timer's
    /// string.
    callback: Option<v8::Local<'s, v8::Value>>,
    /// What few members' calls hold (`RareCall`): one word each call
    /// clears and drops, not a field each (three cost every call about 23
    /// instructions).
    rare: ManuallyDrop<Option<Box<RareCall<'s>>>>,
}

/// The two boxes looked at inline, freed out of line: most calls (every
/// getter's) have neither, and the drop glue's call (its registers saved,
/// two checks) was some 18 instructions each.
impl Drop for Call<'_, '_> {
    #[inline]
    fn drop(&mut self) {
        if self.kept.is_some() || self.rare.is_some() {
            self.drop_held();
        }
    }
}

impl Call<'_, '_> {
    #[cold]
    #[inline(never)]
    fn drop_held(&mut self) {
        // Taken out and dropped here: what stays is `None`, which needs no
        // drop.
        drop((self.kept.take(), self.rare.take()));
    }
}

/// What a call of `whenDefined` or `define` holds.
#[derive(Default)]
pub(crate) struct RareCall<'s> {
    /// What `define` read off its constructor (the lifecycle callbacks:
    /// `connectedCallback`, `disconnectedCallback`, `adoptedCallback`,
    /// `attributeChangedCallback`), for the operation that stores them.
    definition: Option<[Option<v8::Local<'s, v8::Function>>; 4]>,
    /// Whether script threw in the call (a getter `define` ran): the
    /// member's error is that exception, V8's already, not another thrown.
    threw: bool,
    /// `getHTML`'s `shadowRoots`, converted: node wrappers.
    shadow_roots: Vec<v8::Local<'s, v8::Object>>,
    /// An `IntersectionObserverInit`'s `root` or an
    /// `IntersectionObserverEntryInit`'s `target`: a node's wrapper.
    intersection_node: Option<v8::Local<'s, v8::Object>>,
    /// A `StaticRangeInit`'s containers: nodes' wrappers.
    static_range_nodes: Option<[v8::Local<'s, v8::Object>; 2]>,
    /// What a traversal's filter threw, caught (the traversal's flag is
    /// unset in an operation after): the member's error, thrown again.
    thrown: Option<v8::Local<'s, v8::Value>>,
    /// The `Uint8ClampedArray` taken (`Uint8ClampedArg`).
    array: Option<v8::Local<'s, v8::Uint8ClampedArray>>,
    /// The `ArrayBufferView` taken (`ArrayViewArg`).
    view: Option<v8::Local<'s, v8::ArrayBufferView>>,
    /// A second callback taken (`Callback(1)`, `nullable_function_arg`:
    /// geolocation's error callback).
    second_callback: Option<v8::Local<'s, v8::Value>>,
    /// The objects a dictionary's members or a body gave (`KeptObject`'s
    /// index): wrappers.
    kept_objects: Vec<v8::Local<'s, v8::Value>>,
}

impl<'s> Call<'s, '_> {
    fn rare(&mut self) -> &mut RareCall<'s> {
        self.rare.get_or_insert_with(Box::default)
    }
}

/// A member's call in its operation: what of V8 it may use (`NoGc`), the
/// arguments, what the call holds (`Call`, its result by `OpReturn`), and
/// the state's caches it reads.
pub(crate) struct V8Op<'o, 's, 'i, 'r, A: CallArgs<'s>> {
    no_gc: bindings::NoGc<'o, 's, 'i>,
    args: &'o A,
    call: &'o mut Call<'s, 'r>,
    caches: &'o Caches,
}

impl<'a, 's, 'i, 'r, A: CallArgs<'s>> V8Front<'a, 's, 'i, 'r, A> {
    /// `value` kept for the operation (`KeptObject`).
    fn keep_object(&mut self, value: v8::Local<'s, v8::Value>) -> dom_api::typed::KeptObject {
        let kept = &mut self.call.rare().kept_objects;
        let index = kept.len();
        kept.push(value);
        dom_api::typed::KeptObject(index)
    }

    /// `dict`'s member `member`'s value (its getter may throw).
    fn dict_value(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<v8::Local<'s, v8::Value>> {
        let scope = self.scope.scope();
        let key = templates::string(scope, member);
        dict.get(scope, key.into())
    }

    /// `value` (not null or undefined) as `BodyInit`: a blob's or a
    /// `URLSearchParams`' wrapper kept, a buffer's or a view's bytes copied,
    /// anything else its `ToString` (WebIDL's union, a string last).
    fn body_of(&mut self, value: v8::Local<'s, v8::Value>) -> Option<dom_api::typed::BodyArg> {
        use dom_api::typed::BodyArg;
        let scope = self.scope.scope();
        if bindings::is_wrapper::<dom::Blob>(scope, value) {
            return Some(BodyArg::Blob(self.keep_object(value)));
        }
        if bindings::is_wrapper::<dom::UrlSearchParams>(scope, value) {
            return Some(BodyArg::SearchParams(self.keep_object(value)));
        }
        // A copy of a buffer's or a view's bytes, as they are now.
        if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
            let store = buffer.get_backing_store();
            return Some(BodyArg::Bytes(
                store
                    .iter()
                    .take(buffer.byte_length())
                    .map(Cell::get)
                    .collect(),
            ));
        }
        if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
            let mut bytes = vec![0; view.byte_length()];
            let copied = view.copy_contents(&mut bytes);
            bytes.truncate(copied);
            return Some(BodyArg::Bytes(bytes));
        }
        let string = value.to_string(scope)?;
        Some(BodyArg::String(string.to_rust_string_lossy(scope)))
    }

    pub(crate) fn new(
        scope: Scope<'a, 's, 'i>,
        args: &'a A,
        rv: v8::ReturnValue<'r, v8::Value>,
    ) -> Self {
        // Every way into script counts itself (`listeners::run_callback`
        // and the engine's entries): a member called with none counted
        // would run a microtask checkpoint where HTML has none.
        debug_assert!(
            bindings::trace::in_script(),
            "a DOM member is called from script that the engine didn't count as entered"
        );
        Self {
            scope,
            args,
            call: Call {
                rv: bindings::ReturnSlot::new(rv),
                kept: ManuallyDrop::new(None),
                prepared: None,
                window: None,
                callback: None,
                rare: ManuallyDrop::new(None),
            },
        }
    }

    /// Whether the call's `new.target` is `interface`'s interface object
    /// in the current context.
    #[cold]
    #[inline(never)]
    fn is_own_constructor(&mut self, interface: usize) -> bool {
        let new_target = self.args.new_target();
        templates::template(&mut self.scope, interface)
            .get_function(self.scope.scope())
            .is_some_and(|function| v8::Local::<v8::Value>::from(function) == new_target)
    }

    /// A `DOMRectInit` (Geometry's), member `name` of an
    /// `IntersectionObserverEntryInit`: `[x, y, width, height]`, each
    /// unrestricted and 0 if missing; null an empty one. Not an object: a
    /// `TypeError`, thrown.
    fn rect_init(&mut self, value: v8::Local<'s, v8::Value>, name: &str) -> Option<[f64; 4]> {
        if value.is_null() {
            return Some([0.0; 4]);
        }
        let Ok(dict) = v8::Local::<v8::Object>::try_from(value) else {
            self.throw_type_error(&format!(
                "Failed to construct 'IntersectionObserverEntry': Failed to read the '{name}' property from 'IntersectionObserverEntryInit': The provided value is not of type 'DOMRectInit'."
            ));
            return None;
        };
        let scope = self.scope.scope();
        let mut rect = [0.0; 4];
        // By name: `height`, `width`, `x`, `y`.
        for (member, index) in [("height", 3), ("width", 2), ("x", 0), ("y", 1)] {
            let key = templates::string(scope, member);
            let value = dict.get(scope, key.into())?;
            if !value.is_undefined() {
                rect[index] = value.number_value(scope)?;
            }
        }
        Some(rect)
    }

    /// The result whole, out of the operation.
    fn rv(&mut self) -> &mut v8::ReturnValue<'r, v8::Value> {
        self.call.rv.full(self.scope.scope())
    }

    fn throw_type_error(&mut self, message: &str) {
        crate::throw_type_error(self.scope.scope(), message);
    }

    /// Tells V8 of the DOM's growth, after a call that may allocate.
    pub(crate) fn report(&mut self) {
        state::report(&mut self.scope);
    }

    /// The call's result as it stands, for an indexed getter: a null one
    /// is no item, and so is an empty string: CSSOM's `item` isn't
    /// nullable, and gives one past the end (its supported indices are
    /// below `length`); no item of a list of strings is empty.
    pub(crate) fn returned_no_item(&mut self) -> bool {
        let scope = self.scope.scope();
        let value = self.call.rv.full(scope).get(scope);
        value.is_null()
            || v8::Local::<v8::String>::try_from(value).is_ok_and(|string| string.length() == 0)
    }

    /// `kept` made the engine's value, out of any operation: a result
    /// another front end of the same call left (`info_front::InfoFront`).
    pub(crate) fn finish_kept_as(&mut self, kept: Kept) {
        *self.call.kept = Some(Box::new(kept));
        self.finish_kept();
    }

    /// A kept result made the engine's value, after the operation.
    #[inline(never)]
    fn finish_kept(&mut self) {
        let Some(kept) = self.call.kept.take() else {
            return;
        };
        match *kept {
            Kept::Object(object, interface) => {
                let wrapper = wrapper_of(&mut self.scope, &object, interface);
                self.rv().set(wrapper.into());
            }
            Kept::Pixels(image_data, bytes, then) => {
                let len = bytes.len();
                let scope = self.scope.scope();
                let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
                let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
                let array = v8::Uint8ClampedArray::new(scope, buffer, 0, len)
                    .expect("an array of a buffer's length");
                let (state, mut no_gc) = self.scope.state();
                state.dom.read(|cx| {
                    let edge = cx.image_data_array_edge(image_data.get(cx.gc));
                    no_gc.set_strong_value(edge, array.into());
                });
                if let Some(then) = then {
                    *self.call.kept = Some(then);
                    self.finish_kept();
                }
            }
            Kept::Upgrading(root) => {
                let scope = self.scope.scope();
                let prototype = self.args.this().get_prototype(scope);
                let (state, mut no_gc) = self.scope.state();
                let wrapper = state
                    .dom
                    .read(|cx| no_gc.wrapper(cx.wrapper_edge(root.get(cx.gc))))
                    .expect("an element kept for its wrapper");
                if let Some(prototype) = prototype {
                    wrapper.set_prototype(self.scope.scope(), prototype);
                }
                self.rv().set(wrapper.into());
            }
            Kept::Construct(definition, document) => self.finish_construct(&definition, &document),
            Kept::U32(value) => self.rv().set_uint32(value),
            Kept::F64(value) => self.rv().set_double(value),
            Kept::String(string) => match cached_string(&mut self.scope, &string) {
                Some(string) => self.rv().set(string.into()),
                None => exception::throw_range_error(self.scope.scope(), "Invalid string length"),
            },
            Kept::Name(id, name) => match strings::new_string(self.scope.scope(), &name) {
                Some(string) => {
                    let index = usize::try_from(id).expect("a name's number");
                    let global = v8::Global::new(self.scope.scope(), string);
                    let (state, _) = self.scope.state_mut();
                    let names = &mut state.caches.names;
                    if names.len() <= index {
                        names.resize_with(index.checked_add(1).expect("a name's number"), || None);
                    }
                    names[index] = Some(global);
                    state.names_made = state.names_made.wrapping_add(1);
                    self.rv().set(string.into());
                }
                None => exception::throw_range_error(self.scope.scope(), "Invalid string length"),
            },
            Kept::Node(root, interface) => {
                let made = new_wrapper_in_realm(&mut self.scope, interface, |cx| {
                    ObjectRef::Node(root.get(cx.gc))
                });
                let (state, mut no_gc) = self.scope.state();
                // Allocates nothing: a read.
                let wrapper = state.dom.read(|cx| {
                    let node = root.get(cx.gc);
                    // Nothing made it one since (no script ran).
                    match no_gc.wrapper(cx.wrapper_edge(node)) {
                        Some(wrapper) => wrapper,
                        None => {
                            wrap_new(&mut no_gc, cx, made, node);
                            count(&state.caches.wrappers_made);
                            made
                        }
                    }
                });
                self.rv().set(wrapper.into());
            }
        }
    }
}

impl<'s, A: CallArgs<'s>> V8Front<'_, 's, '_, '_, A> {}

impl<'s, A: CallArgs<'s>> V8Front<'_, 's, '_, '_, A> {
    /// The call's result a new promise resolved with `value`.
    fn resolve(&mut self, value: v8::Local<'s, v8::Value>) {
        let scope = self.scope.scope();
        let Some(resolver) = v8::PromiseResolver::new(scope) else {
            return;
        };
        resolver.resolve(scope, value);
        let promise = resolver.get_promise(scope);
        self.rv().set(promise.into());
    }
}

impl<'s, A: CallArgs<'s>> V8Front<'_, 's, '_, '_, A> {
    /// `createElement`'s custom element: its definition's constructor
    /// constructed, in a `TryCatch`, and what it made checked
    /// (`dom_api::check_created_custom_element`); what it threw, or
    /// the check's error, reported at the constructor's realm and a
    /// failed element given instead (HTML's "create an element").
    #[cold]
    #[inline(never)]
    fn finish_construct(
        &mut self,
        definition: &Persistent<dom::CustomElementDefinition>,
        document: &NodeRoot,
    ) {
        if let Some(made) = construct_custom_element(&mut self.scope, definition, document) {
            self.rv().set(made.into());
        }
    }
}

/// HTML's "create an element" with a definition and the synchronous custom
/// elements flag (`createElement`'s, the parser's that will execute
/// script): the definition's constructor constructed, in a `TryCatch`, and
/// what it made checked (`dom_api::check_created_custom_element`);
/// what it threw, or the check's error, reported at the constructor's realm
/// and a failed element made instead. The element's wrapper (none only
/// if the definition has no constructor).
pub(crate) fn construct_custom_element<'s>(
    scope: &mut Scope<'_, 's, '_>,
    definition: &Persistent<dom::CustomElementDefinition>,
    document: &NodeRoot,
) -> Option<v8::Local<'s, v8::Object>> {
    let constructor = constructor_of(scope, definition)?;
    // `HTMLElement`'s constructor finds the definition by it, whichever
    // realm's registry it is of.
    {
        let (state, _) = scope.state_mut();
        state
            .dom
            .mutate(|cx| cx.push_active_constructor(definition.get(cx.gc)));
    }
    let given = {
        v8::tc_scope!(let tc, scope.scope());
        let checked = constructor.new_instance(tc, &[]).and_then(|made| {
            let mut scope = state::scope(tc)?;
            let (state, mut no_gc) = scope.state();
            let checked = state.dom.read(|cx| {
                let api = ApiCx::new(cx);
                let node = no_gc.unwrap::<Node>(cx.gc, made);
                dom_api::check_created_custom_element(
                    &api,
                    node,
                    definition.get(cx.gc),
                    document.get(cx.gc),
                )
                .map(|_| ())
            });
            match checked {
                Ok(()) => Some(made),
                Err(error) => {
                    throw_in(tc, error);
                    None
                }
            }
        });
        if checked.is_none() {
            listeners::report_caught(tc, ReportAt::RealmOf(constructor.into()));
        }
        checked.map(|made| v8::Global::new(tc, made))
    };
    {
        let (state, _) = scope.state_mut();
        state.dom.mutate(|cx| cx.pop_active_constructor());
    }
    if let Some(made) = given {
        return Some(v8::Local::new(scope.scope(), made));
    }
    let (state, _) = scope.state_mut();
    let (root, interface) = state.dom.mutate(|cx| {
        let element = dom_api::failed_custom_element(
            &ApiCx::new(cx),
            definition.get(cx.gc),
            document.get(cx.gc),
        );
        (cx.root(element), interface_of(cx, element))
    });
    let made = templates::new_wrapper(scope, interface);
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        wrap_new(&mut no_gc, cx, made, root.get(cx.gc));
        count(&state.caches.wrappers_made);
    });
    Some(made)
}

/// A dictionary's member's value made a JS value.
fn dict_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: dom_api::typed::DictValue,
) -> v8::Local<'s, v8::Value> {
    match value {
        dom_api::typed::DictValue::Bool(value) => v8::Boolean::new(scope, value).into(),
        dom_api::typed::DictValue::String(value) => templates::string(scope, &value).into(),
        dom_api::typed::DictValue::Number(value) => v8::Number::new(scope, value).into(),
        dom_api::typed::DictValue::Null => v8::null(scope).into(),
    }
}

/// The value an `ApiError` rejects a promise with, made in `scope`.
fn rejection<'s>(
    scope: &mut Scope<'_, 's, '_>,
    error: &ApiError,
) -> Option<v8::Local<'s, v8::Value>> {
    v8::tc_scope!(let tc, scope.scope());
    {
        let mut inner = crate::state::scope(tc)?;
        match error {
            ApiError::Type(message) => crate::throw_type_error(inner.scope(), message),
            ApiError::Range(message) => crate::strings::throw_range_error(inner.scope(), message),
            ApiError::Dom(exception) => exception::throw_dom_exception(
                &mut inner,
                exception.name.name(),
                &exception.message,
            ),
        }
    }
    tc.exception()
}

/// The notices queued for `object`'s promises, applied in order
/// (`dom_api::notice_steps`): to its `ready` or `finished` resolver
/// (`bindings::strong_resolver`), resolved with `object`'s wrapper (of
/// `interface`) or rejected; let go of (written `undefined`) when the next
/// one asked for is new.
pub(crate) fn settle_animation_notices(
    scope: &mut Scope<'_, '_, '_>,
    object: &Persistent<dom::EventTargetImpl>,
) {
    let (notices, interface) = {
        let (state, _) = scope.state();
        state.dom.read(|cx| {
            let target = object.get(cx.gc);
            let interface = dom_api::interface_of(&ApiCx::new(cx), ObjectRef::EventTarget(target));
            (cx.take_animation_notices(target), interface)
        })
    };
    for notice in notices {
        let Some((ready, outcome, forget)) = dom_api::notice_steps(notice) else {
            continue;
        };
        let resolver = {
            let (state, mut no_gc) = scope.state();
            state.dom.read(|cx| {
                let edge = cx.animation_promise_edge(object.get(cx.gc), ready)?;
                no_gc.strong_resolver(edge)
            })
        };
        if let (Some(resolver), Some(outcome)) = (resolver, outcome) {
            match outcome {
                None => {
                    let wrapper =
                        wrapper_of(scope, &ObjectRoot::EventTarget(object.clone()), interface);
                    resolver.resolve(scope.scope(), wrapper.into());
                }
                Some(error) => {
                    if let Some(reason) = rejection(scope, &error) {
                        resolver.reject(scope.scope(), reason);
                    }
                }
            }
        }
        if forget {
            let undefined = v8::undefined(scope.scope());
            let (state, mut no_gc) = scope.state();
            state.dom.read(|cx| {
                if let Some(edge) = cx.animation_promise_edge(object.get(cx.gc), ready) {
                    no_gc.set_strong_value(edge, undefined.into());
                }
            });
        }
    }
}

/// `object`'s animation's `ready` (or `finished`) promise, its notices
/// settled first: the one kept, or a new one kept on it, resolved with its
/// wrapper at once if `resolved`.
fn animation_promise<'s>(
    scope: &mut Scope<'_, 's, '_>,
    object: &Persistent<dom::EventTargetImpl>,
    interface: usize,
    ready: bool,
    resolved: bool,
) -> Option<v8::Local<'s, v8::Promise>> {
    settle_animation_notices(scope, object);
    let kept = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            let edge = cx.animation_promise_edge(object.get(cx.gc), ready)?;
            no_gc.strong_resolver(edge)
        })
    };
    if let Some(resolver) = kept {
        return Some(resolver.get_promise(scope.scope()));
    }
    let resolver = v8::PromiseResolver::new(scope.scope())?;
    {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            if let Some(edge) = cx.animation_promise_edge(object.get(cx.gc), ready) {
                no_gc.set_strong(edge, resolver.into());
            }
        });
    }
    let promise = resolver.get_promise(scope.scope());
    // web-animations 1: an animation's promises are marked as handled
    // (rejected, they report no unhandled rejection).
    promise.mark_as_handled();
    if resolved {
        let wrapper = wrapper_of(scope, &ObjectRoot::EventTarget(object.clone()), interface);
        resolver.resolve(scope.scope(), wrapper.into());
    }
    Some(resolver.get_promise(scope.scope()))
}

/// `history`'s state (`history.state`): the deserialization its edge
/// keeps, or, if it is stale, the state deserialized in the current
/// context (`null` for none, or one that doesn't deserialize) and kept
/// there: the same object until the state changes.
pub(crate) fn history_state<'s>(
    scope: &mut Scope<'_, 's, '_>,
    history: &Persistent<dom::History>,
) -> v8::Local<'s, v8::Value> {
    let kept = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            let history = history.get(cx.gc);
            let (edge, fresh) = cx.history_state_edge(history);
            if fresh {
                Ok(no_gc.strong_value(edge))
            } else {
                Err(cx.history_serialized_state(history))
            }
        })
    };
    let serialized = match kept {
        Ok(Some(value)) => return value,
        Ok(None) => return v8::null(scope.scope()).into(),
        Err(serialized) => serialized,
    };
    let value = serialized
        .and_then(|bytes| {
            crate::serialize::deserialize_message(
                scope.scope(),
                dom::SerializedMessage {
                    bytes,
                    transferred: None,
                },
                Vec::new(),
            )
        })
        .unwrap_or_else(|| v8::null(scope.scope()).into());
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        let history = history.get(cx.gc);
        no_gc.set_strong_value(cx.history_state_edge(history).0, value);
        cx.set_history_state_fresh(history);
    });
    value
}

/// `entry`'s `detail`: the deserialization its edge keeps, or, if it is
/// none yet, its serialization deserialized in the current context (`null`
/// for none) and kept there: the same object each time.
fn entry_detail<'s>(
    scope: &mut Scope<'_, 's, '_>,
    entry: &Persistent<dom::PerformanceEntry>,
) -> v8::Local<'s, v8::Value> {
    let kept = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            let entry = entry.get(cx.gc);
            let (edge, fresh) = cx.performance_entry_detail_edge(entry);
            if fresh {
                Ok(no_gc.strong_value(edge))
            } else {
                Err(cx.performance_entry_detail(entry).map(<[u8]>::to_vec))
            }
        })
    };
    let serialized = match kept {
        Ok(Some(value)) => return value,
        Ok(None) => return v8::null(scope.scope()).into(),
        Err(serialized) => serialized,
    };
    let value = serialized
        .and_then(|bytes| {
            crate::serialize::deserialize_message(
                scope.scope(),
                dom::SerializedMessage {
                    bytes,
                    transferred: None,
                },
                Vec::new(),
            )
        })
        .unwrap_or_else(|| v8::null(scope.scope()).into());
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        let entry = entry.get(cx.gc);
        no_gc.set_strong_value(cx.performance_entry_detail_edge(entry).0, value);
        cx.set_performance_entry_detail_fresh(entry);
    });
    value
}

/// `reader`'s `ArrayBuffer` result: the buffer its edge keeps, or, if it
/// is stale, one made of the result's bytes in the current context and kept
/// there: the same one for the read. `null` if the result is none.
fn reader_buffer<'s>(
    scope: &mut Scope<'_, 's, '_>,
    reader: &Persistent<dom::EventTargetImpl>,
) -> v8::Local<'s, v8::Value> {
    let kept = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            let reader = reader.get(cx.gc);
            let (edge, fresh) = cx.file_reader_buffer_edge(reader);
            if fresh {
                Ok(no_gc.strong_value(edge))
            } else {
                Err(cx.file_reader_result(reader))
            }
        })
    };
    let bytes = match kept {
        Ok(Some(value)) => return value,
        Err(dom::FileReaderResult::Bytes(bytes)) => bytes,
        Ok(None) | Err(_) => return v8::null(scope.scope()).into(),
    };
    let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes.to_vec()).make_shared();
    let buffer: v8::Local<'s, v8::Value> =
        v8::ArrayBuffer::with_backing_store(scope.scope(), &store).into();
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        let reader = reader.get(cx.gc);
        no_gc.set_strong_value(cx.file_reader_buffer_edge(reader).0, buffer);
        cx.set_file_reader_buffer_fresh(reader);
    });
    buffer
}

/// `value` as `(sequence<sequence<S>> or record<S, S> or S)`, `S` a
/// `USVString`, or with `bytes` a `ByteString` (no string branch: the
/// caller checked it is an object; a string with a character past U+00FF
/// a `TypeError`, thrown).
fn string_pairs<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    bytes: bool,
) -> Option<dom_api::StringPairsArg> {
    let convert = |scope: &mut v8::PinScope<'s, '_>,
                   value: v8::Local<'s, v8::Value>|
     -> Option<String> {
        let string = value.to_string(scope)?.to_rust_string_lossy(scope);
        if bytes && string.chars().any(|c| u32::from(c) > 0xFF) {
            crate::throw_type_error(
                scope,
                "Cannot convert to a ByteString because it contains a character outside the range 0-255.",
            );
            return None;
        }
        Some(string)
    };
    let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
        return Some(dom_api::StringPairsArg::String(convert(scope, value)?));
    };
    // WebIDL's union: an object with an `@@iterator` method is the
    // sequence's.
    let symbol = v8::Symbol::get_iterator(scope);
    let method = object.get(scope, symbol.into())?;
    if !method.is_null_or_undefined() {
        let mut items = Vec::new();
        crate::observable_array::for_each_item(scope, value, |scope, item| {
            let mut strings = Vec::new();
            crate::observable_array::for_each_item(scope, item, |scope, string| {
                strings.push(convert(scope, string)?);
                Some(())
            })?;
            items.push(strings);
            Some(())
        })?;
        return Some(dom_api::StringPairsArg::Sequences(items));
    }
    // A record (WebIDL's conversion): for each of its own keys, in order,
    // its property's descriptor read; an enumerable one's key converted (a
    // symbol's a `TypeError`), then its value read and converted.
    let keys = object.get_own_property_names(
        scope,
        v8::GetPropertyNamesArgs {
            mode: v8::KeyCollectionMode::OwnOnly,
            property_filter: v8::PropertyFilter::ALL_PROPERTIES,
            index_filter: v8::IndexFilter::IncludeIndices,
            key_conversion: v8::KeyConversionMode::ConvertToString,
        },
    )?;
    let mut entries = Vec::new();
    for at in 0..keys.length() {
        let key = keys.get_index(scope, at)?;
        let Ok(property) = v8::Local::<v8::Name>::try_from(key) else {
            continue;
        };
        let descriptor = object.get_own_property_descriptor(scope, property)?;
        // Gone since the keys were read.
        let Ok(descriptor) = v8::Local::<v8::Object>::try_from(descriptor) else {
            continue;
        };
        let enumerable = crate::templates::string(scope, "enumerable");
        if !descriptor
            .get(scope, enumerable.into())?
            .boolean_value(scope)
        {
            continue;
        }
        if key.is_symbol() {
            crate::throw_type_error(scope, "Cannot convert a Symbol value to a string");
            return None;
        }
        let name = convert(scope, key)?;
        let item = object.get(scope, key)?;
        let value = convert(scope, item)?;
        // Two keys the same once lone surrogates are replaced: WebIDL's
        // record sets the first's value.
        match entries.iter_mut().find(|(other, _)| *other == name) {
            Some((_, old)) => *old = value,
            None => entries.push((name, value)),
        }
    }
    Some(dom_api::StringPairsArg::Record(entries))
}

/// The DOM's pending promises numbered `promises` (their resolvers kept in
/// the state) resolved with undefined, or rejected with `error` made a
/// JS value once (`ScriptHost::settle_promises`).
/// The DOM's pending promise numbered `promise` (a blob read's, kept by the
/// engine) resolved with `read`'s string, or a new buffer of its bytes
/// (a `Uint8Array` of it for `bytes()`); nothing if its resolver is gone.
pub(crate) fn resolve_promise_with_read(
    scope: &mut Scope<'_, '_, '_>,
    promise: u64,
    read: dom::BlobRead,
) {
    let Some(resolver) = scope.state_mut().0.when_defined.remove(&promise) else {
        return;
    };
    let scope = scope.scope();
    let value: v8::Local<'_, v8::Value> = match read {
        dom::BlobRead::Text(text) => match strings::new_string(scope, &text) {
            Some(string) => string.into(),
            None => {
                let resolver = v8::Local::new(scope, resolver);
                let message =
                    v8::String::new(scope, "Invalid string length").expect("a short string");
                let error = v8::Exception::range_error(scope, message);
                resolver.reject(scope, error);
                return;
            }
        },
        dom::BlobRead::Bytes { bytes, view } => {
            let len = bytes.len();
            let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
            let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
            if view {
                v8::Uint8Array::new(scope, buffer, 0, len)
                    .expect("an array of a buffer's length")
                    .into()
            } else {
                buffer.into()
            }
        }
    };
    let resolver = v8::Local::new(scope, resolver);
    resolver.resolve(scope, value);
}

pub(crate) fn settle_promises(
    scope: &mut Scope<'_, '_, '_>,
    promises: &[u64],
    error: Option<&ApiError>,
) {
    let resolvers: Vec<v8::Global<v8::PromiseResolver>> = {
        let (state, _) = scope.state_mut();
        promises
            .iter()
            .filter_map(|id| state.when_defined.remove(id))
            .collect()
    };
    if resolvers.is_empty() {
        return;
    }
    let reason = error.and_then(|error| {
        v8::tc_scope!(let tc, scope.scope());
        {
            let mut inner = crate::state::scope(tc)?;
            match error {
                ApiError::Type(message) => crate::throw_type_error(inner.scope(), message),
                ApiError::Range(message) => {
                    crate::strings::throw_range_error(inner.scope(), message)
                }
                ApiError::Dom(exception) => exception::throw_dom_exception(
                    &mut inner,
                    exception.name.name(),
                    &exception.message,
                ),
            }
        }
        tc.exception()
    });
    let scope = scope.scope();
    for resolver in resolvers {
        let resolver = v8::Local::new(scope, resolver);
        match reason {
            Some(reason) => resolver.reject(scope, reason),
            None => {
                let undefined = v8::undefined(scope);
                resolver.resolve(scope, undefined.into())
            }
        };
    }
}

/// An upgrade's construction (`ScriptHost::upgrade_element`): the
/// constructor constructed, in a `TryCatch`: what it threw, or a
/// `TypeError` if it gave another object than `element`'s wrapper,
/// reported at its realm.
pub(crate) fn upgrade_element(
    scope: &mut Scope<'_, '_, '_>,
    definition: &Persistent<dom::CustomElementDefinition>,
    element: &Persistent<Node>,
) -> bool {
    let Some(constructor) = constructor_of(scope, definition) else {
        return false;
    };
    let upgradable = {
        let (state, _) = scope.state();
        state.dom.read(|cx| {
            dom_api::check_custom_element_upgradable(
                &ApiCx::new(cx),
                definition.get(cx.gc),
                element.get(cx.gc),
            )
        })
    };
    v8::tc_scope!(let tc, scope.scope());
    if let Err(error) = upgradable {
        throw_in(tc, error);
        listeners::report_caught(tc, ReportAt::RealmOf(constructor.into()));
        return false;
    }
    let Some(made) = constructor.new_instance(tc, &[]) else {
        listeners::report_caught(tc, ReportAt::RealmOf(constructor.into()));
        return false;
    };
    let Some(mut scope) = state::scope(tc) else {
        return false;
    };
    let (state, mut no_gc) = scope.state();
    let same = state
        .dom
        .read(|cx| no_gc.wrapper(cx.wrapper_edge(element.get(cx.gc))))
        .is_some_and(|wrapper| wrapper == made);
    if !same {
        crate::throw_type_error(
            tc,
            "Failed to upgrade: the custom element's constructor did not produce the element being upgraded",
        );
        listeners::report_caught(tc, ReportAt::RealmOf(constructor.into()));
    }
    same
}

/// A lifecycle callback's call (`ScriptHost::call_lifecycle_callback`):
/// the definition's function, `this` the element (its wrapper made if it
/// has none), its arguments (`adoptedCallback`'s documents,
/// `attributeChangedCallback`'s name, values and namespace, null for
/// none), in a `TryCatch`: what it throws reported at its realm.
pub(crate) fn call_lifecycle_callback(
    scope: &mut Scope<'_, '_, '_>,
    element: &Persistent<Node>,
    definition: &Persistent<dom::CustomElementDefinition>,
    callback: dom_api::LifecycleCallback,
) {
    use dom_api::LifecycleCallback as Which;
    let (function, this, documents) = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            let edges = cx.definition_edges(definition.get(cx.gc));
            let edge = match &callback {
                Which::Connected => edges.connected,
                Which::Disconnected => edges.disconnected,
                Which::Adopted(..) => edges.adopted,
                Which::AttributeChanged { .. } => edges.attribute_changed,
            };
            let function = no_gc.strong_value(edge);
            let this = listeners::have(&mut no_gc, cx, ObjectRef::Node(element.get(cx.gc)));
            let documents = match &callback {
                Which::Adopted(old, new) => Some((
                    listeners::have(&mut no_gc, cx, ObjectRef::Node(old.get(cx.gc))),
                    listeners::have(&mut no_gc, cx, ObjectRef::Node(new.get(cx.gc))),
                )),
                _ => None,
            };
            (function, this, documents)
        })
    };
    let Some(function) =
        function.and_then(|function| v8::Local::<v8::Function>::try_from(function).ok())
    else {
        return;
    };
    let this = listeners::wrap(scope, this);
    let mut arguments: Vec<v8::Local<'_, v8::Value>> = Vec::new();
    match callback {
        Which::Connected | Which::Disconnected => {}
        Which::Adopted(..) => {
            let (old, new) = documents.expect("an adoption's documents");
            arguments.push(listeners::wrap(scope, old).into());
            arguments.push(listeners::wrap(scope, new).into());
        }
        Which::AttributeChanged {
            name,
            old,
            new,
            namespace,
        } => {
            for value in [Some(name), old, new, namespace] {
                let value = match value {
                    Some(string) => match strings::new_string(scope.scope(), &string) {
                        Some(string) => string.into(),
                        None => return,
                    },
                    None => v8::null(scope.scope()).into(),
                };
                arguments.push(value);
            }
        }
    }
    v8::tc_scope!(let tc, scope.scope());
    if function.call(tc, this.into(), &arguments).is_none() {
        listeners::report_caught(tc, ReportAt::RealmOf(function.into()));
    }
}

/// `definition`'s constructor, as a function.
fn constructor_of<'s>(
    scope: &mut Scope<'_, 's, '_>,
    definition: &Persistent<dom::CustomElementDefinition>,
) -> Option<v8::Local<'s, v8::Function>> {
    let (state, mut no_gc) = scope.state();
    let value = state
        .dom
        .read(|cx| no_gc.strong_value(cx.definition_edges(definition.get(cx.gc)).constructor))?;
    v8::Local::<v8::Function>::try_from(value).ok()
}

/// What `error` throws, thrown in `tc` (to be caught there).
fn throw_in(tc: &mut v8::PinnedRef<'_, v8::TryCatch<v8::HandleScope>>, error: ApiError) {
    let Some(mut inner) = state::scope(tc) else {
        return;
    };
    match error {
        ApiError::Type(message) => crate::throw_type_error(inner.scope(), &message),
        ApiError::Range(message) => crate::strings::throw_range_error(inner.scope(), &message),
        ApiError::Dom(exception) => {
            exception::throw_dom_exception(&mut inner, exception.name.name(), &exception.message)
        }
    }
}

/// The engine's number for `object` (V8's identity hash, as Blink's
/// `V8CustomElementConstructorHashTraits`), its bits.
pub(crate) fn number(object: v8::Local<'_, v8::Object>) -> u32 {
    u32::from_ne_bytes(object.get_identity_hash().get().to_ne_bytes())
}

/// One more wrapper made, for the tests.
pub(crate) fn count(wrappers_made: &Cell<u64>) {
    wrappers_made.set(wrappers_made.get().wrapping_add(1));
}

/// What `createElement`'s local name says of its element in an HTML
/// document (`dom_api::Front::foresee_element`): by the name's number if
/// it is lowercase ASCII (as most are) and a name already (the element's,
/// lowercased in an HTML document); else `HTMLElement`'s, the wrapper
/// made again after if that isn't it. The name only peeked at: nothing is
/// made (V8 may collect as the wrapper is, after).
pub(crate) fn foresee_element(
    isolate: &v8::Isolate,
    name: &ArgString<'_>,
) -> (Option<dom::NameId>, usize) {
    name.peek_ascii(isolate, |text| {
        if text.bytes().any(|byte| byte.is_ascii_uppercase()) {
            return None;
        }
        Some(match dom::names::find(text) {
            Some(id) => (Some(id), dom_api::html_element_interface(id)),
            // A name no element had yet: as `html_element_interface`
            // says of one past the fixed ones.
            None if dom::is_valid_custom_element_name(text) => {
                (None, dom_api::declare::HTML_ELEMENT)
            }
            None => (None, dom_api::declare::HTML_UNKNOWN_ELEMENT),
        })
    })
    .flatten()
    .unwrap_or((None, dom_api::declare::HTML_ELEMENT))
}

/// `string`'s V8 string, from the isolate's cache, made and cached now if
/// V8 hasn't it (the state let go while V8 makes it); none if it is too
/// long for V8. Out of any operation.
pub(crate) fn cached_string<'s>(
    scope: &mut Scope<'_, 's, '_>,
    string: &DomString,
) -> Option<v8::Local<'s, v8::String>> {
    let dead = {
        let (state, mut no_gc) = scope.state();
        if let Some(local) = state
            .caches
            .strings
            .find(string)
            .and_then(|weak| no_gc.weak_local(weak))
        {
            return Some(local);
        }
        state.caches.strings.dead()
    };
    let (local, weak) = StringCache::make(scope.scope(), dead, string)?;
    scope.state_mut().0.caches.strings.insert(string, weak);
    Some(local)
}

/// The interface a node's wrapper is of: its kind's most derived one, an
/// HTML element's by its local name.
#[inline]
pub(crate) fn interface_of<'gc>(cx: &dom::Cx<'_, 'gc>, node: NodeRef<'gc>) -> usize {
    dom_api::node_interface_of(cx, node)
}

/// `wrapper` (new, wrapping nothing) made `object`'s, in an operation: it
/// holds the object's address, the object holds it.
pub(crate) fn wrap_new_object<'gc, 's>(
    no_gc: &mut bindings::NoGc<'_, 's, '_>,
    cx: &dom::Cx<'_, 'gc>,
    wrapper: v8::Local<'s, v8::Object>,
    object: ObjectRef<'gc>,
) {
    match object {
        ObjectRef::Node(node) => no_gc.wrap(cx.gc, wrapper, node),
        ObjectRef::Event(event) => no_gc.wrap(cx.gc, wrapper, event),
        ObjectRef::EventTarget(target) => no_gc.wrap(cx.gc, wrapper, target),
        ObjectRef::Window(window) => no_gc.wrap(cx.gc, wrapper, window),
        ObjectRef::Location(location) => no_gc.wrap(cx.gc, wrapper, location),
        ObjectRef::History(history) => no_gc.wrap(cx.gc, wrapper, history),
        ObjectRef::Navigator(navigator) => no_gc.wrap(cx.gc, wrapper, navigator),
        ObjectRef::UserActivation(activation) => no_gc.wrap(cx.gc, wrapper, activation),
        ObjectRef::Url(url) => no_gc.wrap(cx.gc, wrapper, url),
        ObjectRef::UrlSearchParams(params) => no_gc.wrap(cx.gc, wrapper, params),
        ObjectRef::Crypto(crypto) => no_gc.wrap(cx.gc, wrapper, crypto),
        ObjectRef::IdleDeadline(deadline) => no_gc.wrap(cx.gc, wrapper, deadline),
        ObjectRef::AbortController(controller) => no_gc.wrap(cx.gc, wrapper, controller),
        ObjectRef::Headers(headers) => no_gc.wrap(cx.gc, wrapper, headers),
        ObjectRef::Request(request) => no_gc.wrap(cx.gc, wrapper, request),
        ObjectRef::Response(response) => no_gc.wrap(cx.gc, wrapper, response),
        ObjectRef::Geolocation(geolocation) => no_gc.wrap(cx.gc, wrapper, geolocation),
        ObjectRef::GeolocationPositionError(error) => no_gc.wrap(cx.gc, wrapper, error),
        ObjectRef::WakeLock(wake_lock) => no_gc.wrap(cx.gc, wrapper, wake_lock),
        ObjectRef::Scheduling(scheduling) => no_gc.wrap(cx.gc, wrapper, scheduling),
        ObjectRef::PerformanceEntry(entry) => no_gc.wrap(cx.gc, wrapper, entry),
        ObjectRef::PerformanceObserver(observer) => no_gc.wrap(cx.gc, wrapper, observer),
        ObjectRef::PerformanceObserverEntryList(list) => no_gc.wrap(cx.gc, wrapper, list),
        ObjectRef::PerformanceTiming(timing) => no_gc.wrap(cx.gc, wrapper, timing),
        ObjectRef::HtmlCollection(collection) => no_gc.wrap(cx.gc, wrapper, collection),
        ObjectRef::NodeList(list) => no_gc.wrap(cx.gc, wrapper, list),
        ObjectRef::DomTokenList(list) => no_gc.wrap(cx.gc, wrapper, list),
        ObjectRef::DomStringMap(map) => no_gc.wrap(cx.gc, wrapper, map),
        ObjectRef::NamedNodeMap(map) => no_gc.wrap(cx.gc, wrapper, map),
        ObjectRef::DomParser(parser) => no_gc.wrap(cx.gc, wrapper, parser),
        ObjectRef::Range(range) => no_gc.wrap(cx.gc, wrapper, range),
        ObjectRef::NodeIterator(iterator) => no_gc.wrap(cx.gc, wrapper, iterator),
        ObjectRef::TreeWalker(walker) => no_gc.wrap(cx.gc, wrapper, walker),
        ObjectRef::Selection(selection) => no_gc.wrap(cx.gc, wrapper, selection),
        ObjectRef::CssStyleDeclaration(declaration) => no_gc.wrap(cx.gc, wrapper, declaration),
        ObjectRef::MutationObserver(observer) => no_gc.wrap(cx.gc, wrapper, observer),
        ObjectRef::MutationRecord(record) => no_gc.wrap(cx.gc, wrapper, record),
        ObjectRef::IntersectionObserver(observer) => no_gc.wrap(cx.gc, wrapper, observer),
        ObjectRef::IntersectionObserverEntry(entry) => no_gc.wrap(cx.gc, wrapper, entry),
        ObjectRef::ResizeObserver(observer) => no_gc.wrap(cx.gc, wrapper, observer),
        ObjectRef::ResizeObserverEntry(entry) => no_gc.wrap(cx.gc, wrapper, entry),
        ObjectRef::ResizeObserverSize(size) => no_gc.wrap(cx.gc, wrapper, size),
        ObjectRef::CanvasRenderingContext2d(context) => no_gc.wrap(cx.gc, wrapper, context),
        ObjectRef::CanvasGradient(gradient) => no_gc.wrap(cx.gc, wrapper, gradient),
        ObjectRef::CanvasPattern(pattern) => no_gc.wrap(cx.gc, wrapper, pattern),
        ObjectRef::ImageData(image_data) => no_gc.wrap(cx.gc, wrapper, image_data),
        ObjectRef::Blob(blob) => no_gc.wrap(cx.gc, wrapper, blob),
        ObjectRef::Path2d(path) => no_gc.wrap(cx.gc, wrapper, path),
        ObjectRef::TextMetrics(metrics) => no_gc.wrap(cx.gc, wrapper, metrics),
        ObjectRef::MessageChannel(channel) => no_gc.wrap(cx.gc, wrapper, channel),
        ObjectRef::SvgAnimatedString(animated) => no_gc.wrap(cx.gc, wrapper, animated),
        ObjectRef::SvgMatrix(matrix) => no_gc.wrap(cx.gc, wrapper, matrix),
        ObjectRef::SvgPoint(point) => no_gc.wrap(cx.gc, wrapper, point),
        ObjectRef::SvgRect(rect) => no_gc.wrap(cx.gc, wrapper, rect),
        ObjectRef::DomRect(rect) => no_gc.wrap(cx.gc, wrapper, rect),
        ObjectRef::DomRectList(list) => no_gc.wrap(cx.gc, wrapper, list),
        ObjectRef::DomImplementation(implementation) => no_gc.wrap(cx.gc, wrapper, implementation),
        ObjectRef::CssStyleSheet(sheet) => no_gc.wrap(cx.gc, wrapper, sheet),
        ObjectRef::CssRule(rule) => no_gc.wrap(cx.gc, wrapper, rule),
        ObjectRef::CssRuleList(list) => no_gc.wrap(cx.gc, wrapper, list),
        ObjectRef::MediaList(list) => no_gc.wrap(cx.gc, wrapper, list),
        ObjectRef::StyleSheetList(list) => no_gc.wrap(cx.gc, wrapper, list),
        ObjectRef::CustomElementRegistry(registry) => no_gc.wrap(cx.gc, wrapper, registry),
        ObjectRef::KeyframeEffect(effect) => no_gc.wrap(cx.gc, wrapper, effect),
        ObjectRef::DocumentTimeline(timeline) => no_gc.wrap(cx.gc, wrapper, timeline),
    }
    no_gc.set_wrapper(cx.object_wrapper_edge(object), wrapper);
}

/// A new wrapper of `interface` for `object` (as `object` finds it), in
/// its realm (`dom_api::realm_document`'s document's: a node another
/// window's script reaches first is still an instance of its own window's
/// interfaces, as Chrome's). The current realm's if that
/// is its document's (most: the one comparison) or no frame shows it (a
/// document of `createHTMLDocument` or `DOMParser`). Blink's
/// `V8WrapperInstantiationScope` enters another context only if it isn't
/// the current one, too.
fn new_wrapper_in_realm<'s>(
    scope: &mut Scope<'_, 's, '_>,
    interface: usize,
    object: impl for<'gc> FnOnce(&dom::Cx<'_, 'gc>) -> ObjectRef<'gc>,
) -> v8::Local<'s, v8::Object> {
    match other_realm(scope, object) {
        Some(context) => {
            let scope = &mut v8::ContextScope::new(scope.scope(), context);
            let mut scope = crate::state::scope(scope).expect("the isolate's state");
            templates::new_wrapper(&mut scope, interface)
        }
        None => templates::new_wrapper(scope, interface),
    }
}

/// The context of `object`'s realm, if it isn't the current one and a
/// frame shows its document.
fn other_realm<'s>(
    scope: &mut Scope<'_, 's, '_>,
    object: impl for<'gc> FnOnce(&dom::Cx<'_, 'gc>) -> ObjectRef<'gc>,
) -> Option<v8::Local<'s, v8::Context>> {
    let global = scope.scope().get_current_context().global(scope.scope());
    let (state, mut no_gc) = scope.state();
    // Allocates nothing: a read.
    let document = state.dom.read(|cx| {
        let document = dom_api::realm_document(cx, object(cx))?;
        let current = no_gc
            .unwrap::<dom::Window>(cx.gc, global)
            .map(|window| cx.window_document(window));
        (!current.is_some_and(|current| elise::Gc::ptr_eq(current, document)))
            .then(|| cx.node_id(document))
    })?;
    let context = &state.frames.of_document(document)?.context;
    Some(no_gc.local(context))
}

/// `object`'s wrapper (kept with its interface), made after the operation
/// unless something made it since.
pub(crate) fn wrapper_of<'s>(
    scope: &mut Scope<'_, 's, '_>,
    object: &ObjectRoot,
    interface: usize,
) -> v8::Local<'s, v8::Object> {
    let made = new_wrapper_in_realm(scope, interface, |cx| object.get(cx.gc));
    let (state, mut no_gc) = scope.state();
    // Allocates nothing: a read.
    state.dom.read(|cx| {
        let object = object.get(cx.gc);
        match no_gc.wrapper(cx.object_wrapper_edge(object)) {
            Some(wrapper) => wrapper,
            None => {
                wrap_new_object(&mut no_gc, cx, made, object);
                count(&state.caches.wrappers_made);
                made
            }
        }
    })
}

/// `wrapper` (new, wrapping nothing) made `node`'s, in an operation: it
/// holds the node's address, the node holds it.
pub(crate) fn wrap_new<'gc, 's>(
    no_gc: &mut bindings::NoGc<'_, 's, '_>,
    cx: &dom::Cx<'_, 'gc>,
    wrapper: v8::Local<'s, v8::Object>,
    node: NodeRef<'gc>,
) {
    no_gc.wrap(cx.gc, wrapper, node);
    no_gc.set_wrapper(cx.wrapper_edge(node), wrapper);
}

/// Argument `index`, undefined past the last.
#[inline]
/// WebIDL's `unsigned long` of `value`: `ToNumber`, then its integer part
/// modulo 2^32 (`NaN` and the infinities 0); `None` if it threw. A number
/// is V8's `Uint32Value`'s; anything else spelled out: V8's doesn't wrap a
/// string's negative number (`"-1"` is 0).
pub(crate) fn to_uint32(
    scope: &v8::PinScope<'_, '_>,
    value: v8::Local<'_, v8::Value>,
) -> Option<u32> {
    if value.is_number() {
        return value.uint32_value(scope);
    }
    let number = value.number_value(scope)?;
    Some(if number.is_finite() {
        base::numerics::saturated_cast(number.trunc().rem_euclid(4_294_967_296.0))
    } else {
        0
    })
}

pub(crate) fn arg<'s>(args: &impl CallArgs<'s>, index: usize) -> v8::Local<'s, v8::Value> {
    args.get(i32::try_from(index).unwrap_or(i32::MAX))
}

impl<'s, A: CallArgs<'s>> FrontTypes for V8Front<'_, 's, '_, '_, A> {
    type Str = ArgString<'s>;
    type Node = v8::Local<'s, v8::Object>;
    type Dict = v8::Local<'s, v8::Object>;
    type PendingString = std::convert::Infallible;
    type PendingNode = std::convert::Infallible;
    type PendingNumber = std::convert::Infallible;
    type PendingObject = std::convert::Infallible;
    type PreparedNode = ();
    /// Which of the call\'s kept callbacks (`Call::callback`, or the
    /// second: `RareCall::second_callback`).
    type Callback = usize;
    type Any = v8::Local<'s, v8::Value>;
    /// The arguments in a new array.
    type Rest = v8::Local<'s, v8::Array>;
}

impl<'s, A: CallArgs<'s>> FrontTypes for V8Op<'_, 's, '_, '_, A> {
    type Str = ArgString<'s>;
    type Node = v8::Local<'s, v8::Object>;
    type Dict = v8::Local<'s, v8::Object>;
    type PendingString = std::convert::Infallible;
    type PendingNode = std::convert::Infallible;
    type PendingNumber = std::convert::Infallible;
    type PendingObject = std::convert::Infallible;
    type PreparedNode = ();
    /// Which of the call\'s kept callbacks (`Call::callback`, or the
    /// second: `RareCall::second_callback`).
    type Callback = usize;
    type Any = v8::Local<'s, v8::Value>;
    /// The arguments in a new array.
    type Rest = v8::Local<'s, v8::Array>;
}

impl<'a, 's, 'i, 'r, A: CallArgs<'s>> Front for V8Front<'a, 's, 'i, 'r, A> {
    type Op<'o>
        = V8Op<'o, 's, 'i, 'r, A>
    where
        Self: 'o;

    #[inline]
    fn read<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R {
        let (state, no_gc) = self.scope.state();
        let mut op = V8Op {
            no_gc,
            args: self.args,
            call: &mut self.call,
            caches: &state.caches,
        };
        state
            .dom
            .read(|cx| f(&ApiCx::with_layout_cell(cx, &state.layout), &mut op))
    }

    #[inline]
    fn mutate<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R {
        let (state, no_gc) = self.scope.state_mut();
        let State {
            dom,
            caches,
            layout,
            ..
        } = state;
        let mut op = V8Op {
            no_gc,
            args: self.args,
            call: &mut self.call,
            caches,
        };
        dom.mutate(|cx| f(&ApiCx::with_layout_cell(cx, layout), &mut op))
    }

    #[inline]
    fn has_args(&mut self, name: &str, required: usize) -> bool {
        let given = usize::try_from(self.args.length()).unwrap_or(0);
        if given >= required {
            return true;
        }
        self.throw_type_error(&format!(
            "Failed to execute '{name}': {required} argument{} required, but only {given} present.",
            if required == 1 { "" } else { "s" }
        ));
        false
    }

    fn string_arg(&mut self, index: usize) -> Option<ArgString<'s>> {
        strings::arg_string(self.scope.scope(), arg(self.args, index))
    }

    fn arg_count(&mut self) -> usize {
        usize::try_from(self.args.length()).unwrap_or(0)
    }

    fn nullable_string_arg(&mut self, index: usize) -> Option<Option<ArgString<'s>>> {
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        strings::arg_string(self.scope.scope(), value).map(Some)
    }

    fn legacy_null_string_arg(&mut self, index: usize) -> Option<Option<ArgString<'s>>> {
        let value = arg(self.args, index);
        if value.is_null() {
            return Some(None);
        }
        strings::arg_string(self.scope.scope(), value).map(Some)
    }

    fn node_or_string_arg(
        &mut self,
        index: usize,
    ) -> Option<dom_api::typed::NodeOrStr<v8::Local<'s, v8::Object>, ArgString<'s>>> {
        let value = arg(self.args, index);
        if bindings::is_wrapper::<Node>(self.scope.scope(), value) {
            return v8::Local::<v8::Object>::try_from(value)
                .ok()
                .map(dom_api::typed::NodeOrStr::Node);
        }
        strings::arg_string(self.scope.scope(), value).map(dom_api::typed::NodeOrStr::Str)
    }

    fn node_or_long_arg(
        &mut self,
        index: usize,
    ) -> Option<Option<dom_api::typed::NodeOrLong<v8::Local<'s, v8::Object>>>> {
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if bindings::is_wrapper::<Node>(self.scope.scope(), value) {
            return v8::Local::<v8::Object>::try_from(value)
                .ok()
                .map(|node| Some(dom_api::typed::NodeOrLong::Node(node)));
        }
        value
            .int32_value(self.scope.scope())
            .map(|long| Some(dom_api::typed::NodeOrLong::Long(long)))
    }

    fn node_arg(&mut self, index: usize) -> Option<Option<v8::Local<'s, v8::Object>>> {
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if bindings::is_wrapper::<Node>(self.scope.scope(), value) {
            return v8::Local::<v8::Object>::try_from(value).ok().map(Some);
        }
        self.throw_type_error(&format!(
            "parameter {} is not of type 'Node'.",
            index.saturating_add(1)
        ));
        None
    }

    fn dict_arg(&mut self, index: usize, name: &str) -> Option<Option<v8::Local<'s, v8::Object>>> {
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
            return Some(Some(object));
        }
        self.throw_type_error(&format!("The provided value is not of type '{name}'."));
        None
    }

    /// The member's value (a getter may run, and throw: V8 has the
    /// exception pending then), `ToBoolean`ed.
    fn dict_bool(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<bool>> {
        let scope = self.scope.scope();
        let key = templates::string(scope, member);
        let value = dict.get(scope, key.into())?;
        if value.is_undefined() {
            return Some(None);
        }
        Some(Some(value.boolean_value(scope)))
    }

    fn dict_number(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<f64>> {
        let scope = self.scope.scope();
        let key = templates::string(scope, member);
        let value = dict.get(scope, key.into())?;
        if value.is_undefined() {
            return Some(None);
        }
        Some(Some(value.number_value(scope)?))
    }

    /// Cold: only a constructor's dictionary reads one (`KeyboardEvent`'s),
    /// and placed among the hot calls it made `appendChild` and
    /// `dispatchEvent` a third slower (the same instructions).
    #[cold]
    fn dict_string(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<String>> {
        let scope = self.scope.scope();
        let key = templates::string(scope, member);
        let value = dict.get(scope, key.into())?;
        if value.is_undefined() {
            return Some(None);
        }
        let string = value.to_string(scope)?;
        Some(Some(string.to_rust_string_lossy(scope)))
    }

    /// Cold, as `dict_string`: only `InputEvent`'s constructor reads one.
    #[cold]
    fn dict_nullable_string(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<String>> {
        let scope = self.scope.scope();
        let key = templates::string(scope, member);
        let value = dict.get(scope, key.into())?;
        if value.is_null_or_undefined() {
            return Some(None);
        }
        let string = value.to_string(scope)?;
        Some(Some(string.to_rust_string_lossy(scope)))
    }

    fn dict_serialized(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<Vec<u8>>> {
        let scope = self.scope.scope();
        let key = templates::string(scope, member);
        let value = dict.get(scope, key.into())?;
        if value.is_undefined() {
            return Some(None);
        }
        crate::serialize::serialize_for_storage(scope, value).map(Some)
    }

    fn dict_string_or_number(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<dom_api::typed::StringOrNumber>> {
        let scope = self.scope.scope();
        let key = templates::string(scope, member);
        let value = dict.get(scope, key.into())?;
        if value.is_undefined() {
            return Some(None);
        }
        if value.is_number() {
            return Some(Some(dom_api::typed::StringOrNumber::Number(
                value.number_value(scope)?,
            )));
        }
        let string = value.to_string(scope)?.to_rust_string_lossy(scope);
        Some(Some(dom_api::typed::StringOrNumber::String(string)))
    }

    fn dict_any(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<AnyArg<v8::Local<'s, v8::Value>>>> {
        let scope = self.scope.scope();
        let key = templates::string(scope, member);
        let value = dict.get(scope, key.into())?;
        if value.is_undefined() {
            return Some(None);
        }
        Some(Some(AnyArg(value)))
    }

    fn any_arg(&mut self, index: usize) -> AnyArg<v8::Local<'s, v8::Value>> {
        let mut value = arg(self.args, index);
        if value.is_undefined() {
            value = v8::null(self.scope.scope()).into();
        }
        AnyArg(value)
    }

    fn target_arg(
        &mut self,
        index: usize,
        interface: &str,
    ) -> Option<Option<v8::Local<'s, v8::Object>>> {
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        let scope = self.scope.scope();
        if bindings::is_wrapper::<Node>(scope, value)
            || bindings::is_wrapper::<Window>(scope, value)
            || bindings::is_wrapper::<EventTargetImpl>(scope, value)
        {
            return v8::Local::<v8::Object>::try_from(value).ok().map(Some);
        }
        self.throw_type_error(&format!(
            "parameter {} is not of type '{interface}'.",
            index.saturating_add(1)
        ));
        None
    }

    /// An array's elements, each `ToString`ed (WebIDL's `sequence`
    /// conversion of an array; another iterable is a `TypeError` here).
    fn dict_strings(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<Vec<String>>> {
        let scope = self.scope.scope();
        let key = templates::string(scope, member);
        let value = dict.get(scope, key.into())?;
        if value.is_undefined() {
            return Some(None);
        }
        let Ok(array) = v8::Local::<v8::Array>::try_from(value) else {
            self.throw_type_error(&format!(
                "The provided value for '{member}' is not a sequence."
            ));
            return None;
        };
        let mut strings = Vec::new();
        for index in 0..array.length() {
            let element = array.get_index(self.scope.scope(), index)?;
            strings.push(
                element
                    .to_string(self.scope.scope())?
                    .to_rust_string_lossy(self.scope.scope()),
            );
        }
        Some(Some(strings))
    }

    fn function_arg(&mut self, index: usize, name: &str) -> Option<Callback<usize>> {
        let value = arg(self.args, index);
        if value.is_function() {
            assert!(self.call.callback.is_none(), "a member takes one callback");
            self.call.callback = Some(value);
            return Some(Callback(0));
        }
        self.throw_type_error(&format!(
            "parameter {} is not of type '{name}'.",
            index.saturating_add(1)
        ));
        None
    }

    fn uint8_clamped_arg(&mut self, index: usize) -> Option<Uint8ClampedArg> {
        let value = arg(self.args, index);
        let Ok(array) = v8::Local::<v8::Uint8ClampedArray>::try_from(value) else {
            self.throw_type_error(&format!(
                "parameter {} is not of type 'Uint8ClampedArray'.",
                index.saturating_add(1)
            ));
            return None;
        };
        let len = array.byte_length();
        self.call.rare().array = Some(array);
        Some(Uint8ClampedArg { len })
    }

    fn array_view_arg(&mut self, index: usize) -> Option<dom_api::typed::ArrayViewArg> {
        let value = arg(self.args, index);
        let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) else {
            self.throw_type_error(&format!(
                "parameter {} is not of type 'ArrayBufferView'.",
                index.saturating_add(1)
            ));
            return None;
        };
        let integer = [
            ("Int8Array", value.is_int8_array()),
            ("Uint8Array", value.is_uint8_array()),
            ("Uint8ClampedArray", value.is_uint8_clamped_array()),
            ("Int16Array", value.is_int16_array()),
            ("Uint16Array", value.is_uint16_array()),
            ("Int32Array", value.is_int32_array()),
            ("Uint32Array", value.is_uint32_array()),
            ("BigInt64Array", value.is_big_int64_array()),
            ("BigUint64Array", value.is_big_uint64_array()),
        ];
        let other = [
            ("Float32Array", value.is_float32_array()),
            ("Float64Array", value.is_float64_array()),
            ("DataView", value.is_data_view()),
        ];
        let (name, integer) = match integer.iter().find(|(_, is)| *is) {
            Some((name, _)) => (*name, true),
            // A `Float16Array`'s too.
            None => (
                other
                    .iter()
                    .find(|(_, is)| *is)
                    .map_or("Float16Array", |(name, _)| *name),
                false,
            ),
        };
        let byte_length = view.byte_length();
        self.call.rare().view = Some(view);
        Some(dom_api::typed::ArrayViewArg {
            name,
            integer,
            byte_length,
        })
    }

    fn clone_arg(
        &mut self,
        index: usize,
    ) -> Option<(dom::SerializedMessage, Vec<v8::Local<'s, v8::Object>>)> {
        let value = arg(self.args, index);
        let options = arg(self.args, index.saturating_add(1));
        crate::serialize::serialize_message(
            &mut self.scope,
            value,
            options,
            crate::serialize::TransferForm::Options,
            None,
        )
    }

    fn string_pairs_arg(&mut self, index: usize) -> Option<dom_api::StringPairsArg> {
        let value = arg(self.args, index);
        string_pairs(self.scope.scope(), value, false)
    }

    fn byte_string_pairs_arg(&mut self, index: usize) -> Option<dom_api::StringPairsArg> {
        let value = arg(self.args, index);
        if !value.is_object() {
            self.throw_type_error(
                "The provided value is not of type '(record<ByteString, ByteString> or sequence<sequence<ByteString>>)'.",
            );
            return None;
        }
        string_pairs(self.scope.scope(), value, true)
    }

    /// The value, or a new `AbortError` of the current realm for
    /// undefined (Blink's `AbortController::abort` makes it then).
    fn abort_reason_arg(&mut self, index: usize) -> Option<AnyArg<v8::Local<'s, v8::Value>>> {
        let mut value = arg(self.args, index);
        if value.is_undefined() {
            value = exception::new_dom_exception(
                &mut self.scope,
                "AbortError",
                "signal is aborted without reason",
            )?
            .into();
        }
        Some(AnyArg(value))
    }

    fn objects_arg<T: elise::HeapType>(
        &mut self,
        index: usize,
        interface: &str,
    ) -> Option<Vec<v8::Local<'s, v8::Object>>> {
        let value = arg(self.args, index);
        let values = crate::observable_array::sequence(self.scope.scope(), value)?;
        let mut objects = Vec::with_capacity(values.len());
        for value in values {
            match v8::Local::<v8::Object>::try_from(value) {
                Ok(object) if bindings::is_wrapper::<T>(self.scope.scope(), value) => {
                    objects.push(object)
                }
                _ => {
                    self.throw_type_error(&format!(
                        "parameter {}: Failed to convert value to '{interface}'.",
                        index.saturating_add(1)
                    ));
                    return None;
                }
            }
        }
        Some(objects)
    }

    /// The member's value itself (`addEventListener`'s `signal`).
    fn dict_object<T: elise::HeapType>(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
        interface: &str,
    ) -> Option<Option<v8::Local<'s, v8::Object>>> {
        let scope = self.scope.scope();
        let key = templates::string(scope, member);
        let value = dict.get(scope, key.into())?;
        if value.is_undefined() {
            return Some(None);
        }
        if !bindings::is_wrapper::<T>(self.scope.scope(), value) {
            self.throw_type_error(&format!(
                "Failed to read the '{member}' property: Failed to convert value to '{interface}'."
            ));
            return None;
        }
        Some(v8::Local::<v8::Object>::try_from(value).ok())
    }

    fn dict_nullable_object(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
        kind: dom_api::typed::KeptKind,
    ) -> Option<Option<dom_api::typed::KeptObject>> {
        use dom_api::typed::KeptKind;
        let value = self.dict_value(dict, member)?;
        if value.is_null_or_undefined() {
            return Some(None);
        }
        let scope = self.scope.scope();
        let (accepted, interface) = match kind {
            KeptKind::Window => (bindings::is_wrapper::<Window>(scope, value), "Window"),
            KeptKind::EventTarget => (
                bindings::is_wrapper::<Node>(scope, value)
                    || bindings::is_wrapper::<Window>(scope, value)
                    || bindings::is_wrapper::<EventTargetImpl>(scope, value),
                "EventTarget",
            ),
        };
        if !accepted {
            self.throw_type_error(&format!(
                "Failed to read the '{member}' property: Failed to convert value to '{interface}'."
            ));
            return None;
        }
        Some(Some(self.keep_object(value)))
    }

    fn body_arg(&mut self, index: usize) -> Option<Option<dom_api::typed::BodyArg>> {
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        self.body_of(value).map(Some)
    }

    fn dict_body(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<dom_api::typed::BodyMember>> {
        let value = self.dict_value(dict, member)?;
        if value.is_undefined() {
            return Some(None);
        }
        if value.is_null() {
            return Some(Some(dom_api::typed::BodyMember::Null));
        }
        Some(Some(dom_api::typed::BodyMember::Body(self.body_of(value)?)))
    }

    fn dict_headers(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<dom_api::StringPairsArg>> {
        let value = self.dict_value(dict, member)?;
        if value.is_undefined() {
            return Some(None);
        }
        if !value.is_object() {
            self.throw_type_error(&format!(
                "Failed to read the '{member}' property: The provided value is not of type '(record<ByteString, ByteString> or sequence<sequence<ByteString>>)'."
            ));
            return None;
        }
        string_pairs(self.scope.scope(), value, true).map(Some)
    }

    fn dict_signal(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<dom_api::typed::SignalMember>> {
        let value = self.dict_value(dict, member)?;
        if value.is_undefined() {
            return Some(None);
        }
        if value.is_null() {
            return Some(Some(dom_api::typed::SignalMember::Null));
        }
        if !bindings::is_wrapper::<dom::EventTargetImpl>(self.scope.scope(), value) {
            self.throw_type_error(&format!(
                "Failed to read the '{member}' property: Failed to convert value to 'AbortSignal'."
            ));
            return None;
        }
        Some(Some(dom_api::typed::SignalMember::Signal(
            self.keep_object(value),
        )))
    }

    fn dict_is_null(
        &mut self,
        dict: &v8::Local<'s, v8::Object>,
        member: &str,
    ) -> Option<Option<bool>> {
        let value = self.dict_value(dict, member)?;
        Some((!value.is_undefined()).then(|| value.is_null()))
    }

    fn json_arg(&mut self, index: usize) -> Option<String> {
        let value = arg(self.args, index);
        let scope = self.scope.scope();
        let json = v8::json::stringify(scope, value)?.to_rust_string_lossy(scope);
        // What `JSON.stringify` gives undefined for (undefined, a
        // function, a symbol), V8's API gives as this, which no JSON text
        // is.
        if json == "undefined" {
            self.throw_type_error("The data is not JSON serializable");
            return None;
        }
        Some(json)
    }

    fn object_if<T: elise::HeapType>(&mut self, index: usize) -> Option<v8::Local<'s, v8::Object>> {
        let value = arg(self.args, index);
        if bindings::is_wrapper::<T>(self.scope.scope(), value) {
            return v8::Local::<v8::Object>::try_from(value).ok();
        }
        None
    }

    /// WebIDL's sequence conversion, each item as a `BlobPart` union is:
    /// a blob's wrapper, else a buffer or a view of one (its bytes copied
    /// now, as File API's "process blob parts" takes them), else a string.
    fn blob_parts_arg(
        &mut self,
        index: usize,
    ) -> Option<Option<Vec<BlobPartArg<v8::Local<'s, v8::Object>>>>> {
        let value = arg(self.args, index);
        if value.is_undefined() {
            return Some(None);
        }
        let mut parts = Vec::new();
        crate::observable_array::for_each_item(self.scope.scope(), value, |scope, value| {
            let part = if bindings::is_wrapper::<dom::Blob>(scope, value) {
                BlobPartArg::Blob(v8::Local::<v8::Object>::try_from(value).ok()?)
            } else if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
                let store = buffer.get_backing_store();
                BlobPartArg::Bytes(
                    store
                        .iter()
                        .take(buffer.byte_length())
                        .map(Cell::get)
                        .collect(),
                )
            } else if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
                let mut bytes = vec![0; view.byte_length()];
                let copied = view.copy_contents(&mut bytes);
                bytes.truncate(copied);
                BlobPartArg::Bytes(bytes)
            } else {
                BlobPartArg::String(value.to_string(scope)?.to_rust_string_lossy(scope))
            };
            parts.push(part);
            Some(())
        })?;
        Some(Some(parts))
    }

    fn message_arg(
        &mut self,
        index: usize,
        transfer: dom_api::TransferArg,
        from_port: bool,
    ) -> Option<(dom::SerializedMessage, Vec<v8::Local<'s, v8::Object>>)> {
        let value = arg(self.args, index);
        let (at, form) = match transfer {
            dom_api::TransferArg::Sequence(at) => (at, crate::serialize::TransferForm::Sequence),
            dom_api::TransferArg::Options(at) => (at, crate::serialize::TransferForm::Options),
            dom_api::TransferArg::SequenceOrOptions(at) => {
                (at, crate::serialize::TransferForm::SequenceOrOptions)
            }
        };
        let transfer = arg(self.args, at);
        let source = from_port.then(|| self.args.this());
        crate::serialize::serialize_message(&mut self.scope, value, transfer, form, source)
    }

    fn serialized_state_arg(&mut self, index: usize) -> Option<Vec<u8>> {
        let value = arg(self.args, index);
        crate::serialize::serialize_for_storage(self.scope.scope(), value)
    }

    fn animation_options_arg(&mut self, index: usize) -> Option<dom_api::typed::AnimationOptions> {
        let value = arg(self.args, index);
        crate::keyframes::read_options(self.scope.scope(), value, true)
    }

    fn effect_options_arg(&mut self, index: usize) -> Option<dom_api::typed::AnimationOptions> {
        let value = arg(self.args, index);
        crate::keyframes::read_options(self.scope.scope(), value, false)
    }

    fn optional_timing_arg(&mut self, index: usize) -> Option<animation::OptionalTiming> {
        let value = arg(self.args, index);
        crate::keyframes::read_optional_timing(self.scope.scope(), value)
    }

    fn timeline_arg(&mut self, index: usize) -> Option<bool> {
        let value = arg(self.args, index);
        crate::keyframes::read_timeline(self.scope.scope(), value)
    }

    fn keyframes_arg(&mut self, index: usize) -> Option<Option<animation::ScriptKeyframes>> {
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        Some(Some(crate::keyframes::read(self.scope.scope(), value)?))
    }

    fn round_rect_radii_arg(&mut self, index: usize) -> Option<Vec<Radius>> {
        let value = arg(self.args, index);
        if value.is_undefined() {
            return Some(vec![Radius::Both(0.0)]);
        }
        let scope = self.scope.scope();
        if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
            let symbol = v8::Symbol::get_iterator(scope);
            if !object.get(scope, symbol.into())?.is_undefined() {
                let mut radii = Vec::new();
                crate::observable_array::for_each_item(scope, value, |scope, item| {
                    radii.push(radius(scope, item)?);
                    Some(())
                })?;
                return Some(radii);
            }
        }
        Some(vec![radius(scope, value)?])
    }

    fn bool_if(&mut self, index: usize) -> Option<bool> {
        let value = arg(self.args, index);
        value
            .is_boolean()
            .then(|| value.boolean_value(self.scope.scope()))
    }

    fn number_if(&mut self, index: usize) -> Option<f64> {
        let value = arg(self.args, index);
        value
            .is_number()
            .then(|| value.number_value(self.scope.scope()))
            .flatten()
    }

    fn doubles_arg(&mut self, index: usize) -> Option<Vec<f64>> {
        let value = arg(self.args, index);
        let values = crate::observable_array::sequence(self.scope.scope(), value)?;
        values
            .into_iter()
            .map(|value| value.number_value(self.scope.scope()))
            .collect()
    }

    fn bool_arg(&mut self, index: usize) -> bool {
        arg(self.args, index).boolean_value(self.scope.scope())
    }

    fn is_undefined(&mut self, index: usize) -> bool {
        arg(self.args, index).is_undefined()
    }

    fn is_null_or_undefined(&mut self, index: usize) -> bool {
        arg(self.args, index).is_null_or_undefined()
    }

    fn is_dictionary_like(&mut self, index: usize) -> bool {
        let value = arg(self.args, index);
        value.is_null_or_undefined() || value.is_object()
    }

    /// Any object (a function, or one with `handleEvent`, read when it is
    /// called).
    fn callback_arg(&mut self, index: usize, name: &str) -> Option<Option<Callback<usize>>> {
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if value.is_object() {
            assert!(self.call.callback.is_none(), "a member takes one callback");
            self.call.callback = Some(value);
            return Some(Some(Callback(0)));
        }
        self.throw_type_error(&format!(
            "parameter {} is not of type '{name}'.",
            index.saturating_add(1)
        ));
        None
    }

    /// A function kept as the call's second callback, none for null or
    /// undefined.
    fn nullable_function_arg(
        &mut self,
        index: usize,
        name: &str,
    ) -> Option<Option<Callback<usize>>> {
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if value.is_function() {
            self.call.rare().second_callback = Some(value);
            return Some(Some(Callback(1)));
        }
        self.throw_type_error(&format!(
            "parameter {} is not of type '{name}'.",
            index.saturating_add(1)
        ));
        None
    }

    /// Any object kept (callable or not: `[LegacyTreatNonObjectAsNull]`),
    /// anything else null.
    fn event_handler_arg(&mut self, index: usize) -> Option<Callback<usize>> {
        let value = arg(self.args, index);
        if !value.is_object() {
            return None;
        }
        assert!(self.call.callback.is_none(), "a member takes one callback");
        self.call.callback = Some(value);
        Some(Callback(0))
    }

    /// A function as it is, anything else its `ToString` (which may run
    /// script, before the operation).
    fn timer_handler_arg(&mut self, index: usize) -> Option<Callback<usize>> {
        let value = arg(self.args, index);
        let handler = if value.is_function() {
            value
        } else {
            value.to_string(self.scope.scope())?.into()
        };
        assert!(self.call.callback.is_none(), "a member takes one callback");
        self.call.callback = Some(handler);
        Some(Callback(0))
    }

    fn i32_arg(&mut self, index: usize) -> Option<i32> {
        arg(self.args, index).int32_value(self.scope.scope())
    }

    fn u32_arg(&mut self, index: usize) -> Option<u32> {
        to_uint32(self.scope.scope(), arg(self.args, index))
    }

    fn f64_arg(&mut self, index: usize, finite: bool) -> Option<f64> {
        let value = arg(self.args, index).number_value(self.scope.scope())?;
        if finite && !value.is_finite() {
            self.throw_type_error("The provided double value is non-finite.");
            return None;
        }
        Some(value)
    }

    /// A new array, made before the operation (the engine's heap isn't
    /// touched in one).
    fn rest_arg(&mut self, index: usize) -> Option<v8::Local<'s, v8::Array>> {
        let len = usize::try_from(self.args.length()).unwrap_or(0);
        if len <= index {
            return None;
        }
        let values: Vec<_> = (index..len).map(|index| arg(self.args, index)).collect();
        Some(v8::Array::new_with_elements(self.scope.scope(), &values))
    }

    fn object_arg<T: elise::HeapType>(
        &mut self,
        index: usize,
        interface: &str,
    ) -> Option<Option<v8::Local<'s, v8::Object>>> {
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some(None);
        }
        if bindings::is_wrapper::<T>(self.scope.scope(), value) {
            return v8::Local::<v8::Object>::try_from(value).ok().map(Some);
        }
        self.throw_type_error(&format!(
            "parameter {} is not of type '{interface}'.",
            index.saturating_add(1)
        ));
        None
    }

    fn construct(&mut self, interface: usize, required: usize) -> bool {
        let name = dom_api::INTERFACES[interface].name;
        if self.args.new_target().is_undefined() {
            self.throw_type_error(&format!(
                "Failed to construct '{name}': Please use the 'new' operator, this DOM object constructor cannot be called as a function."
            ));
            return false;
        }
        // `[HTMLConstructor]`: `new.target` is `HTMLElement` itself (its
        // own constructor, or one defined with it), as Blink compares it
        // with its realm's interface object.
        if interface == const { dom_api::declare::interface_index("HTMLElement") }
            && self.is_own_constructor(interface)
        {
            self.throw_type_error("Illegal constructor");
            return false;
        }
        let given = usize::try_from(self.args.length()).unwrap_or(0);
        if given >= required {
            return true;
        }
        self.throw_type_error(&format!(
            "Failed to construct '{name}': {required} argument{} required, but only {given} present.",
            if required == 1 { "" } else { "s" }
        ));
        false
    }

    /// The current context's global, whose window (if it is one of the new
    /// DOM's: its global proxy wraps it) the operation reads.
    fn get_html_options(&mut self, index: usize) -> Option<(bool, bool)> {
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some((false, false));
        }
        let Ok(dict) = v8::Local::<v8::Object>::try_from(value) else {
            self.throw_type_error(
                "Failed to execute 'getHTML': The provided value is not of type 'GetHTMLOptions'.",
            );
            return None;
        };
        let scope = self.scope.scope();
        let key = templates::string(scope, "serializableShadowRoots");
        let serializable = dict.get(scope, key.into())?.boolean_value(scope);
        let key = templates::string(scope, "shadowRoots");
        let listed = dict.get(scope, key.into())?;
        if listed.is_undefined() {
            return Some((serializable, false));
        }
        let mut nodes = Vec::new();
        for item in crate::observable_array::sequence(scope, listed)? {
            match v8::Local::<v8::Object>::try_from(item) {
                Ok(object) if bindings::is_wrapper::<Node>(scope, item) => nodes.push(object),
                _ => {
                    self.throw_type_error("Failed to execute 'getHTML': Failed to read the 'shadowRoots' property from 'GetHTMLOptions': Failed to convert value to 'ShadowRoot'.");
                    return None;
                }
            }
        }
        self.call.rare().shadow_roots = nodes;
        Some((serializable, true))
    }

    fn intersection_observer_init(
        &mut self,
        index: usize,
    ) -> Option<dom_api::typed::IntersectionInit> {
        const NAME: &str = "Failed to construct 'IntersectionObserver'";
        let mut init = dom_api::typed::IntersectionInit::default();
        let value = arg(self.args, index);
        if value.is_null_or_undefined() {
            return Some(init);
        }
        let Ok(dict) = v8::Local::<v8::Object>::try_from(value) else {
            self.throw_type_error(&format!(
                "{NAME}: The provided value is not of type 'IntersectionObserverInit'."
            ));
            return None;
        };
        // The members in the order WebIDL reads them: by name.
        let scope = self.scope.scope();
        let member = |scope: &mut v8::PinScope<'s, '_>, name: &str| {
            let key = templates::string(scope, name);
            dict.get(scope, key.into())
        };
        let number = |scope: &mut v8::PinScope<'s, '_>, value: v8::Local<'s, v8::Value>| {
            value.number_value(scope)
        };
        let delay = member(scope, "delay")?;
        if !delay.is_undefined() {
            init.delay = number(scope, delay)?;
            if !init.delay.is_finite() {
                self.throw_type_error(&format!(
                    "{NAME}: Failed to read the 'delay' property from 'IntersectionObserverInit': The provided double value is non-finite."
                ));
                return None;
            }
        }
        let scope = self.scope.scope();
        let root = member(scope, "root")?;
        if !root.is_null_or_undefined() {
            match v8::Local::<v8::Object>::try_from(root) {
                Ok(object) if bindings::is_wrapper::<Node>(scope, root) => {
                    init.root = true;
                    self.call.rare().intersection_node = Some(object);
                }
                _ => {
                    self.throw_type_error(&format!(
                        "{NAME}: Failed to read the 'root' property from 'IntersectionObserverInit': The provided value is not of type '(Element or Document)'."
                    ));
                    return None;
                }
            }
        }
        for (name, out) in [
            ("rootMargin", &mut init.root_margin),
            ("scrollMargin", &mut init.scroll_margin),
        ] {
            let scope = self.scope.scope();
            let value = member(scope, name)?;
            if !value.is_undefined() {
                *out = value.to_string(scope)?.to_rust_string_lossy(scope);
            }
        }
        let scope = self.scope.scope();
        let threshold = member(scope, "threshold")?;
        if !threshold.is_undefined() {
            // `(double or sequence<double>)`: an object is the sequence.
            let values = if threshold.is_object() {
                crate::observable_array::sequence(scope, threshold)?
            } else {
                vec![threshold]
            };
            init.thresholds.clear();
            for value in values {
                let value = number(self.scope.scope(), value)?;
                if !value.is_finite() {
                    self.throw_type_error(&format!(
                        "{NAME}: Failed to read the 'threshold' property from 'IntersectionObserverInit': The provided double value is non-finite."
                    ));
                    return None;
                }
                init.thresholds.push(value);
            }
        }
        let scope = self.scope.scope();
        let track = member(scope, "trackVisibility")?;
        init.track_visibility = track.boolean_value(scope);
        Some(init)
    }

    fn intersection_entry_init(
        &mut self,
        index: usize,
    ) -> Option<dom_api::typed::IntersectionEntryInitValues> {
        const NAME: &str = "Failed to construct 'IntersectionObserverEntry'";
        const INIT: &str = "IntersectionObserverEntryInit";
        let value = arg(self.args, index);
        // A dictionary: undefined and null are an empty one, whose
        // required members are missing.
        let dict = if value.is_null_or_undefined() {
            v8::Object::new(self.scope.scope())
        } else if let Ok(dict) = v8::Local::<v8::Object>::try_from(value) {
            dict
        } else {
            self.throw_type_error(&format!(
                "{NAME}: The provided value is not of type '{INIT}'."
            ));
            return None;
        };
        // The members in the order WebIDL reads them: by name; each one
        // required.
        let mut values = Vec::with_capacity(8);
        for name in [
            "boundingClientRect",
            "intersectionRatio",
            "intersectionRect",
            "isIntersecting",
            "isVisible",
            "rootBounds",
            "target",
            "time",
        ] {
            let scope = self.scope.scope();
            let key = templates::string(scope, name);
            let value = dict.get(scope, key.into())?;
            if value.is_undefined() {
                self.throw_type_error(&format!(
                    "{NAME}: Failed to read the '{name}' property from '{INIT}': Required member is undefined."
                ));
                return None;
            }
            let converted = match name {
                "boundingClientRect" | "intersectionRect" | "rootBounds" => {
                    if name == "rootBounds" && value.is_null() {
                        Init::Rect(None)
                    } else {
                        Init::Rect(Some(self.rect_init(value, name)?))
                    }
                }
                "isIntersecting" | "isVisible" => {
                    Init::Bool(value.boolean_value(self.scope.scope()))
                }
                "target" => {
                    let scope = self.scope.scope();
                    match v8::Local::<v8::Object>::try_from(value) {
                        Ok(object) if bindings::is_wrapper::<Node>(scope, value) => {
                            self.call.rare().intersection_node = Some(object);
                            Init::Bool(true)
                        }
                        _ => {
                            self.throw_type_error(&format!(
                                "{NAME}: Failed to read the 'target' property from '{INIT}': Failed to convert value to 'Element'."
                            ));
                            return None;
                        }
                    }
                }
                _ => {
                    let number = value.number_value(self.scope.scope())?;
                    if !number.is_finite() {
                        self.throw_type_error(&format!(
                            "{NAME}: Failed to read the '{name}' property from '{INIT}': The provided double value is non-finite."
                        ));
                        return None;
                    }
                    Init::Number(number)
                }
            };
            values.push(converted);
        }
        let [
            Init::Rect(Some(bounding_client_rect)),
            Init::Number(intersection_ratio),
            Init::Rect(Some(intersection_rect)),
            Init::Bool(is_intersecting),
            Init::Bool(is_visible),
            Init::Rect(root_bounds),
            Init::Bool(_),
            Init::Number(time),
        ] = values[..]
        else {
            unreachable!("each member converted as its type")
        };
        Some(dom_api::typed::IntersectionEntryInitValues {
            time,
            root_bounds,
            bounding_client_rect,
            intersection_rect,
            is_intersecting,
            is_visible,
            intersection_ratio,
        })
    }

    fn static_range_init(&mut self, index: usize) -> Option<(u32, u32)> {
        const NAME: &str = "Failed to construct 'StaticRange'";
        const INIT: &str = "StaticRangeInit";
        let value = arg(self.args, index);
        // A dictionary: undefined and null are an empty one, whose
        // required members are missing.
        let dict = if value.is_null_or_undefined() {
            v8::Object::new(self.scope.scope())
        } else if let Ok(dict) = v8::Local::<v8::Object>::try_from(value) {
            dict
        } else {
            self.throw_type_error(&format!(
                "{NAME}: The provided value is not of type '{INIT}'."
            ));
            return None;
        };
        // The members in the order WebIDL reads them: by name; each one
        // required.
        let mut nodes = Vec::with_capacity(2);
        let mut offsets = Vec::with_capacity(2);
        for name in ["endContainer", "endOffset", "startContainer", "startOffset"] {
            let scope = self.scope.scope();
            let key = templates::string(scope, name);
            let value = dict.get(scope, key.into())?;
            if value.is_undefined() {
                self.throw_type_error(&format!(
                    "{NAME}: Failed to read the '{name}' property from '{INIT}': Required member is undefined."
                ));
                return None;
            }
            if name.ends_with("Offset") {
                offsets.push(to_uint32(scope, value)?);
                continue;
            }
            match v8::Local::<v8::Object>::try_from(value) {
                Ok(object) if bindings::is_wrapper::<Node>(scope, value) => nodes.push(object),
                _ => {
                    self.throw_type_error(&format!(
                        "{NAME}: Failed to read the '{name}' property from '{INIT}': Failed to convert value to 'Node'."
                    ));
                    return None;
                }
            }
        }
        // Read end first: the start's are the second.
        self.call.rare().static_range_nodes = Some([nodes[1], nodes[0]]);
        Some((offsets[1], offsets[0]))
    }

    fn prepare_window(&mut self) {
        let scope = self.scope.scope();
        let context = scope.get_current_context();
        self.call.window = Some(context.global(scope));
    }

    fn prepare_relevant_window(&mut self) {
        let scope = self.scope.scope();
        let this = self.args.this();
        let context = this
            .get_creation_context(scope)
            .unwrap_or_else(|| scope.get_current_context());
        self.call.window = Some(context.global(scope));
    }

    fn prepare_incumbent_window(&mut self) {
        let scope = self.scope.scope();
        let context = scope.get_incumbent_context();
        self.call.window = Some(v8::Local::new(scope, context).global(scope));
    }

    fn prepare_entry_window(&mut self) {
        let scope = self.scope.scope();
        let context = scope.get_entered_or_microtask_context();
        self.call.window = Some(v8::Local::new(scope, context).global(scope));
    }

    fn prepare_new_node(&mut self, interface: usize) {
        // A method of another realm called on this one's object (another
        // frame's `Document.prototype.createTextNode` on this document):
        // the node's wrapper is made after, in its relevant realm (its
        // document's, as Blink's bindings convert a result with the
        // receiver's `ScriptState`); the one prepared here would be the
        // current realm's.
        let scope = self.scope.scope();
        if self
            .args
            .this()
            .get_creation_context(scope)
            .is_some_and(|context| context != scope.get_current_context())
        {
            return;
        }
        let wrapper = templates::new_wrapper(&mut self.scope, interface);
        self.call.prepared = Some((wrapper, interface));
    }

    fn foresee_element(&mut self, name: &ArgString<'s>) -> (Option<dom::NameId>, usize) {
        foresee_element(self.scope.scope(), name)
    }

    /// Its tag alone, mostly (the result was returned in the operation),
    /// inlined: the rest out of line, whose registers saved first would
    /// cost every call.
    #[inline]
    fn finish(&mut self) {
        if self.call.kept.is_some() {
            self.finish_kept();
        }
    }

    /// Out of line: the members' calls mostly don't throw, and its
    /// registers saved inline would cost each.
    #[cold]
    #[inline(never)]
    fn throw(&mut self, error: ApiError) {
        if self.call.rare.as_ref().is_some_and(|rare| rare.threw) {
            return;
        }
        if let Some(thrown) = self.call.rare.as_mut().and_then(|rare| rare.thrown.take()) {
            self.scope.scope().throw_exception(thrown);
            return;
        }
        match error {
            ApiError::Type(message) => self.throw_type_error(&message),
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

    /// A new promise rejected with what `error` throws (WebIDL's for an
    /// operation giving a promise).
    #[cold]
    #[inline(never)]
    fn reject(&mut self, error: ApiError) {
        // What the member gave to throw (an aborted signal's reason).
        let thrown = self.call.rare.as_mut().and_then(|rare| rare.thrown.take());
        let reason = if let Some(thrown) = thrown {
            Some(thrown)
        } else {
            v8::tc_scope!(let tc, self.scope.scope());
            {
                let Some(mut inner) = crate::state::scope(tc) else {
                    return;
                };
                match error {
                    ApiError::Type(message) => crate::throw_type_error(inner.scope(), &message),
                    ApiError::Range(message) => {
                        crate::strings::throw_range_error(inner.scope(), &message)
                    }
                    ApiError::Dom(exception) => {
                        exception::throw_dom_exception(
                            &mut inner,
                            exception.name.name(),
                            &exception.message,
                        );
                    }
                }
            }
            tc.exception()
        };
        let scope = self.scope.scope();
        let (Some(reason), Some(resolver)) = (reason, v8::PromiseResolver::new(scope)) else {
            return;
        };
        resolver.reject(scope, reason);
        let promise = resolver.get_promise(scope);
        self.rv().set(promise.into());
    }

    fn constructor_is_constructor(&mut self) -> bool {
        let taken = self.call.callback.expect("the constructor taken");
        v8::Local::<v8::Object>::try_from(taken).is_ok_and(|object| object.is_constructor())
    }

    fn constructor_is_defined(
        &mut self,
        registry: &Persistent<dom::CustomElementRegistry>,
    ) -> bool {
        self.read(|cx, op| {
            let number = op.callback_number(Callback(0));
            cx.custom_element_definitions_numbered(registry.get(cx.gc), number)
                .into_iter()
                .any(|definition| {
                    op.same_callback(cx, cx.definition_edges(definition).constructor, Callback(0))
                })
        })
    }

    fn read_custom_element_constructor(&mut self) -> Result<ConstructorInfo, ApiError> {
        let constructor =
            v8::Local::<v8::Object>::try_from(self.call.callback.expect("the constructor taken"))
                .expect("a constructor, checked");
        match crate::custom_elements::read_constructor(self.scope.scope(), constructor) {
            Some((callbacks, info)) => {
                self.call.rare().definition = Some(callbacks);
                Ok(info)
            }
            None => {
                // V8's exception is the member's: no other thrown over it.
                self.call.rare().threw = true;
                Err(ApiError::type_error(
                    "define: the constructor's reading threw",
                ))
            }
        }
    }

    fn store_custom_element_definition(
        &mut self,
        registry: &Persistent<dom::CustomElementRegistry>,
        definition: &Persistent<dom::CustomElementDefinition>,
    ) {
        let callbacks = self
            .call
            .rare()
            .definition
            .take()
            .expect("the callbacks read");
        self.mutate(|cx, op| {
            let edges = cx.definition_edges(definition.get(cx.gc));
            let constructor =
                v8::Local::<v8::Object>::try_from(op.call.callback.expect("the constructor taken"))
                    .expect("a constructor, checked");
            op.no_gc.set_strong(edges.constructor, constructor);
            cx.set_definition_constructor_number(
                registry.get(cx.gc),
                definition.get(cx.gc),
                number(constructor),
            );
            let callback_edges = [
                (edges.connected, dom::Callback::Connected),
                (edges.disconnected, dom::Callback::Disconnected),
                (edges.adopted, dom::Callback::Adopted),
                (edges.attribute_changed, dom::Callback::AttributeChanged),
            ];
            let mut has = 0;
            for ((edge, which), callback) in callback_edges.into_iter().zip(callbacks) {
                if let Some(callback) = callback {
                    op.no_gc.set_strong(edge, callback.into());
                    has |= which.bit();
                }
            }
            cx.set_definition_callbacks(definition.get(cx.gc), has);
        });
    }

    /// The constructor constructed, in a `TryCatch`: what it threw, or a
    /// `TypeError` if it gave another object than `element`'s wrapper,
    /// reported at its realm.
    fn upgrade_element(
        &mut self,
        definition: &Persistent<dom::CustomElementDefinition>,
        element: &Persistent<Node>,
    ) -> bool {
        crate::listeners::call_constructor(&mut self.scope, |scope| {
            upgrade_element(scope, definition, element)
        })
    }

    fn call_lifecycle_callback(
        &mut self,
        element: &Persistent<Node>,
        definition: &Persistent<dom::CustomElementDefinition>,
        callback: dom_api::LifecycleCallback,
    ) {
        crate::listeners::call_function(&mut self.scope, |scope| {
            call_lifecycle_callback(scope, element, definition, callback)
        });
    }

    fn pending_input(&mut self, include_continuous: bool) -> Vec<dom_api::PendingInput> {
        crate::listeners::pending_input(&mut self.scope, include_continuous)
    }

    fn reject_promise_with_reason(
        &mut self,
        promise: u64,
        signal: &Persistent<dom::EventTargetImpl>,
    ) {
        crate::listeners::reject_promise_with_reason(&mut self.scope, promise, signal);
    }

    fn resolve_when_defined(
        &mut self,
        pending: &[u64],
        definition: &Persistent<dom::CustomElementDefinition>,
    ) {
        let _ = definition;
        let constructor = self.call.callback.expect("the constructor taken");
        for id in pending {
            let (state, _) = self.scope.state_mut();
            let Some(resolver) = state.when_defined.remove(id) else {
                continue;
            };
            let scope = self.scope.scope();
            let resolver = v8::Local::new(scope, resolver);
            resolver.resolve(scope, constructor);
        }
    }

    fn call_listener(&mut self, event: &Persistent<dom::Event>, index: usize) {
        crate::listeners::call_listener(&mut self.scope, event, index, false);
    }

    fn call_node_filter(
        &mut self,
        traversal: &dom::TraversalRoot,
        node: &NodeRoot,
    ) -> Result<u16, ApiError> {
        let filtered = crate::listeners::call_function(&mut self.scope, |scope| {
            crate::listeners::call_node_filter(scope, traversal, node)
        });
        match filtered {
            Ok(result) => Ok(result),
            Err(thrown) => {
                self.call.rare().thrown = Some(thrown);
                Err(ApiError::type_error("the filter threw"))
            }
        }
    }

    fn compile_event_handler(&mut self, target: &dom::TargetRoot, event_type: dom::NameId) {
        crate::handlers::compile_event_handler(&mut self.scope, target, event_type);
    }

    // The results the engine allocates for, made after the operation
    // (`RetType::finish`): each the call's result.

    /// A new promise, its resolver kept by its number; or the one whose
    /// resolver is kept by it already.
    #[cold]
    fn finish_pending_promise(&mut self, id: u64, existing: bool) {
        let resolver = if existing {
            let (state, _) = self.scope.state_mut();
            let kept = state
                .when_defined
                .get(&id)
                .cloned()
                .expect("the promise kept by its number");
            v8::Local::new(self.scope.scope(), kept)
        } else {
            let Some(resolver) = v8::PromiseResolver::new(self.scope.scope()) else {
                return;
            };
            let global = v8::Global::new(self.scope.scope(), resolver);
            let (state, _) = self.scope.state_mut();
            state.when_defined.insert(id, global);
            resolver
        };
        let promise = resolver.get_promise(self.scope.scope());
        self.rv().set(promise.into());
    }

    #[cold]
    fn finish_resolved_constructor(
        &mut self,
        definition: Persistent<dom::CustomElementDefinition>,
    ) {
        let (state, mut no_gc) = self.scope.state();
        // Allocates nothing: a read.
        let constructor = state
            .dom
            .read(|cx| no_gc.strong_value(cx.definition_edges(definition.get(cx.gc)).constructor))
            .expect("a definition's constructor, stored as it was added");
        self.resolve(constructor);
    }

    fn finish_objects(&mut self, objects: Vec<(ObjectRoot, usize)>) {
        let wrappers: Vec<v8::Local<'_, v8::Value>> = objects
            .iter()
            .map(|(object, interface)| wrapper_of(&mut self.scope, object, *interface).into())
            .collect();
        let array = v8::Array::new_with_elements(self.scope.scope(), &wrappers);
        self.rv().set(array.into());
    }

    fn finish_frozen_objects(&mut self, objects: Vec<(ObjectRoot, usize)>) {
        let wrappers: Vec<v8::Local<'_, v8::Value>> = objects
            .iter()
            .map(|(object, interface)| wrapper_of(&mut self.scope, object, *interface).into())
            .collect();
        let this = self.args.this();
        let array = frozen_array(self.scope.scope(), this, &wrappers);
        self.rv().set(array.into());
    }

    fn finish_strings(&mut self, strings: Vec<DomString>) {
        let mut values: Vec<v8::Local<'_, v8::Value>> = Vec::with_capacity(strings.len());
        for string in &strings {
            let Some(value) = strings::new_string(self.scope.scope(), string) else {
                exception::throw_range_error(self.scope.scope(), "Invalid string length");
                return;
            };
            values.push(value.into());
        }
        let array = v8::Array::new_with_elements(self.scope.scope(), &values);
        self.rv().set(array.into());
    }

    fn finish_frozen_strings(&mut self, strings: Vec<DomString>) {
        let this = self.args.this();
        let mut values: Vec<v8::Local<'_, v8::Value>> = Vec::with_capacity(strings.len());
        for string in &strings {
            let Some(value) = strings::new_string(self.scope.scope(), string) else {
                exception::throw_range_error(self.scope.scope(), "Invalid string length");
                return;
            };
            values.push(value.into());
        }
        let array = frozen_array(self.scope.scope(), this, &values);
        self.rv().set(array.into());
    }

    fn finish_doubles(&mut self, numbers: Vec<f64>) {
        let scope = self.scope.scope();
        let values: Vec<v8::Local<'_, v8::Value>> = numbers
            .iter()
            .map(|&number| v8::Number::new(scope, number).into())
            .collect();
        let array = v8::Array::new_with_elements(scope, &values);
        self.rv().set(array.into());
    }

    fn finish_numbers(&mut self, numbers: Vec<f64>) {
        let this = self.args.this();
        let scope = self.scope.scope();
        let values: Vec<v8::Local<'_, v8::Value>> = numbers
            .iter()
            .map(|&number| v8::Number::new(scope, number).into())
            .collect();
        let array = frozen_array(scope, this, &values);
        self.rv().set(array.into());
    }

    fn finish_dictionary(&mut self, members: Vec<(&'static str, dom_api::typed::DictValue)>) {
        let scope = self.scope.scope();
        let object = v8::Object::new(scope);
        for (name, value) in members {
            let key = templates::string(scope, name);
            let value = dict_value(scope, value);
            object.create_data_property(scope, key.into(), value);
        }
        self.rv().set(object.into());
    }

    fn finish_records(&mut self, records: Vec<Vec<(String, dom_api::typed::DictValue)>>) {
        let scope = self.scope.scope();
        let array = v8::Array::new(scope, 0);
        for (index, record) in (0u32..).zip(records) {
            let object = v8::Object::new(scope);
            for (name, value) in record {
                let key = templates::string(scope, &name);
                let value = dict_value(scope, value);
                object.create_data_property(scope, key.into(), value);
            }
            array.set_index(scope, index, object.into());
        }
        self.rv().set(array.into());
    }

    fn finish_entry_detail(&mut self, entry: Persistent<dom::PerformanceEntry>) {
        let detail = entry_detail(&mut self.scope, &entry);
        self.rv().set(detail);
    }

    fn finish_reader_buffer(&mut self, reader: Persistent<dom::EventTargetImpl>) {
        let buffer = reader_buffer(&mut self.scope, &reader);
        self.rv().set(buffer);
    }

    fn finish_history_state(&mut self, history: Persistent<dom::History>) {
        let state = history_state(&mut self.scope, &history);
        self.rv().set(state);
    }

    fn finish_filled_view(&mut self, bytes: Vec<u8>) {
        let view = self
            .call
            .rare()
            .view
            .take()
            .expect("a view kept with the call");
        let scope = self.scope.scope();
        let offset = view.byte_offset();
        if let Some(buffer) = view.buffer(scope) {
            let store = buffer.get_backing_store();
            for (cell, byte) in store.iter().skip(offset).zip(bytes) {
                cell.set(byte);
            }
        }
        self.rv().set(view.into());
    }

    fn finish_cloned(&mut self, payload: dom::SerializedMessage, ports: Vec<(ObjectRoot, usize)>) {
        // In `this`'s relevant realm (the window's), its ports'
        // wrappers too.
        let this = self.args.this();
        let value = {
            let scope = self.scope.scope();
            let context = this
                .get_creation_context(scope)
                .unwrap_or_else(|| scope.get_current_context());
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut realm = state::scope(scope).expect("the isolate's state");
            let ports: Vec<_> = ports
                .iter()
                .map(|(port, interface)| {
                    let wrapper = wrapper_of(&mut realm, port, *interface);
                    v8::Global::new(realm.scope(), wrapper)
                })
                .collect();
            crate::serialize::deserialize_message(realm.scope(), payload, ports)
                .map(|value| v8::Global::new(realm.scope(), value))
        };
        match value {
            Some(value) => {
                let value = v8::Local::new(self.scope.scope(), value);
                self.rv().set(value);
            }
            None => exception::throw_dom_exception(
                &mut self.scope,
                "DataCloneError",
                "Failed to execute 'structuredClone' on 'Window': Unable to deserialize cloned data.",
            ),
        }
    }

    fn finish_resolved(&mut self, object: ObjectRoot, interface: usize) {
        let wrapper = wrapper_of(&mut self.scope, &object, interface);
        let scope = self.scope.scope();
        let Some(resolver) = v8::PromiseResolver::new(scope) else {
            return;
        };
        resolver.resolve(scope, wrapper.into());
        let promise = resolver.get_promise(scope);
        self.rv().set(promise.into());
    }

    fn finish_resolved_string(&mut self, string: DomString) {
        let scope = self.scope.scope();
        let Some(string) = strings::new_string(scope, &string) else {
            exception::throw_range_error(scope, "Invalid string length");
            return;
        };
        self.resolve(string.into());
    }

    fn finish_resolved_json(&mut self, text: DomString) {
        let scope = self.scope.scope();
        let Some(resolver) = v8::PromiseResolver::new(scope) else {
            return;
        };
        let Some(string) = strings::new_string(scope, &text) else {
            exception::throw_range_error(scope, "Invalid string length");
            return;
        };
        // Parsed, or what parsing threw (a `SyntaxError`), caught.
        let parsed = {
            v8::tc_scope!(let tc, scope);
            match v8::json::parse(tc, string) {
                Some(value) => Ok(value),
                None => Err(tc.exception()),
            }
        };
        let scope = self.scope.scope();
        match parsed {
            Ok(value) => {
                resolver.resolve(scope, value);
            }
            Err(Some(thrown)) => {
                resolver.reject(scope, thrown);
            }
            Err(None) => {}
        }
        let promise = resolver.get_promise(scope);
        self.rv().set(promise.into());
    }

    fn finish_resolved_undefined(&mut self) {
        let undefined = v8::undefined(self.scope.scope());
        self.resolve(undefined.into());
    }

    fn finish_resolved_bytes(&mut self, bytes: Vec<u8>, view: bool) {
        let len = bytes.len();
        let scope = self.scope.scope();
        let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
        let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
        let value: v8::Local<'_, v8::Value> = if view {
            v8::Uint8Array::new(scope, buffer, 0, len)
                .expect("an array of a buffer's length")
                .into()
        } else {
            buffer.into()
        };
        self.resolve(value);
    }

    fn finish_animation_promise(
        &mut self,
        object: Persistent<dom::EventTargetImpl>,
        interface: usize,
        ready: bool,
        resolved: bool,
    ) {
        if let Some(promise) =
            animation_promise(&mut self.scope, &object, interface, ready, resolved)
        {
            self.rv().set(promise.into());
        }
    }

    fn update_elements(&mut self) {
        crate::state::update_elements(&mut self.scope);
    }
}

impl<'s, A: CallArgs<'s>> V8Op<'_, 's, '_, '_, A> {
    /// A node with no wrapper kept, its wrapper made after the operation:
    /// out of line, so that a getter whose node has one (most) doesn't
    /// carry the interface's lookup (an element's name read, a table).
    #[cold]
    #[inline(never)]
    fn keep_node<'gc>(&mut self, cx: &dom::Cx<'_, 'gc>, node: NodeRef<'gc>) {
        self.keep(Kept::Node(cx.root(node), interface_of(cx, node)));
    }

    /// `kept` kept for after the operation: out of line, so that a result
    /// returned at once (most) carries no allocation, and its setter
    /// inlines.
    #[cold]
    #[inline(never)]
    fn keep(&mut self, kept: Kept) {
        // After an `ImageData`'s pixels: the result, once they are its.
        if let Some(pixels) = &mut *self.call.kept
            && let Kept::Pixels(_, _, then) = &mut **pixels
        {
            *then = Some(Box::new(kept));
            return;
        }
        *self.call.kept = Some(Box::new(kept));
    }
}

impl<'s, A: CallArgs<'s>> OpFront for V8Op<'_, 's, '_, '_, A> {
    fn str_param<'a>(&mut self, string: &'a ArgString<'s>) -> &'a str {
        string.text(&mut self.no_gc)
    }

    /// V8's signature checked the receiver is a wrapper of the member's
    /// interface (`CallArgs::receiver`): its map isn't read again, only its
    /// tag, which says of which type it is. Another call's is checked
    /// whole. Always inlined: each member's call has one, and LLVM left
    /// it out of line, its call costing more than the map's check saved.
    #[inline(always)]
    fn this_as<'gc, T: elise::HeapType>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
    ) -> Option<elise::Gc<'gc, T>> {
        match self.args.receiver() {
            Some(this) => self.no_gc.unwrap_receiver::<T>(cx.gc, this),
            None => self.no_gc.unwrap::<T>(cx.gc, self.args.this()),
        }
    }

    fn node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, node: &v8::Local<'s, v8::Object>) -> NodeRef<'gc> {
        self.no_gc
            .unwrap::<Node>(cx.gc, *node)
            .expect("a wrapper, checked as it was taken")
    }

    fn object<'gc, T: elise::HeapType>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: &v8::Local<'s, v8::Object>,
    ) -> elise::Gc<'gc, T> {
        self.no_gc
            .unwrap::<T>(cx.gc, *object)
            .expect("a wrapper, checked as it was taken")
    }

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

    fn store_any(
        &mut self,
        _: &ApiCx<'_, '_>,
        edge: &JsEdge,
        value: AnyArg<v8::Local<'s, v8::Value>>,
    ) {
        self.no_gc.set_strong_value(edge, value.0);
    }

    /// Thrown after the operation, as a traversal's filter's is
    /// (`RareCall::thrown`).
    fn rethrow(&mut self, _: &ApiCx<'_, '_>, edge: &JsEdge) -> ApiError {
        if let Some(value) = self.no_gc.strong_value(edge) {
            self.call.rare().thrown = Some(value);
        }
        ApiError::type_error("the value thrown")
    }

    fn kept_object<'gc, T: elise::HeapType>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: dom_api::typed::KeptObject,
    ) -> elise::Gc<'gc, T> {
        let value = *self
            .call
            .rare
            .as_ref()
            .and_then(|rare| rare.kept_objects.get(object.0))
            .expect("the object kept");
        let object =
            v8::Local::<v8::Object>::try_from(value).expect("a wrapper, checked as it was kept");
        self.no_gc
            .unwrap::<T>(cx.gc, object)
            .expect("a wrapper, checked as it was kept")
    }

    fn kept_target<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: dom_api::typed::KeptObject,
    ) -> TargetRef<'gc> {
        let value = *self
            .call
            .rare
            .as_ref()
            .and_then(|rare| rare.kept_objects.get(object.0))
            .expect("the object kept");
        let object =
            v8::Local::<v8::Object>::try_from(value).expect("a wrapper, checked as it was kept");
        self.target(cx, &object)
    }

    fn store_array(&mut self, _: &ApiCx<'_, '_>, edge: &JsEdge, array: Uint8ClampedArg) {
        let Uint8ClampedArg { .. } = array;
        let taken = self
            .call
            .rare
            .as_ref()
            .and_then(|rare| rare.array)
            .expect("the array taken");
        self.no_gc.set_strong_value(edge, taken.into());
    }

    /// In place, unless V8 keeps the bytes on its heap (a small array):
    /// copied to the stack then.
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

    fn set_image_data_pixels<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        image_data: elise::Gc<'gc, dom::ImageData>,
        bytes: Vec<u8>,
    ) {
        let then = self.call.kept.take();
        *self.call.kept = Some(Box::new(Kept::Pixels(
            Persistent::new(cx.gc, image_data),
            bytes,
            then,
        )));
    }

    fn store_rest(&mut self, _: &ApiCx<'_, '_>, edge: &JsEdge, rest: v8::Local<'s, v8::Array>) {
        self.no_gc.set_strong_value(edge, rest.into());
    }

    fn same_callback(
        &mut self,
        _: &ApiCx<'_, '_>,
        edge: &JsEdge,
        callback: Callback<usize>,
    ) -> bool {
        let _ = callback;
        let taken = self.call.callback.expect("the callback taken");
        self.no_gc
            .wrapper(edge)
            .is_some_and(|held| v8::Local::<v8::Value>::from(held) == taken)
    }

    /// An object as an object's strong edge, a timer's string as any
    /// value's.
    fn store_callback(&mut self, _: &ApiCx<'_, '_>, edge: &JsEdge, callback: Callback<usize>) {
        let taken = if callback.0 == 1 {
            self.call
                .rare
                .as_ref()
                .and_then(|rare| rare.second_callback)
        } else {
            self.call.callback
        }
        .expect("the callback taken");
        match v8::Local::<v8::Object>::try_from(taken) {
            Ok(object) => self.no_gc.set_strong(edge, object),
            Err(_) => self.no_gc.set_strong_value(edge, taken),
        }
    }

    /// The current context's global's window (`Front::prepare_window`).
    fn current_window<'gc>(&mut self, cx: &ApiCx<'_, 'gc>) -> Option<elise::Gc<'gc, dom::Window>> {
        let global = self.call.window?;
        self.no_gc.unwrap::<dom::Window>(cx.gc, global)
    }

    /// The call's receiver, made from the interface's instance template
    /// (with `new.target`'s prototype, a subclass's), wrapping nothing: V8
    /// returns it.
    fn construct_object<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, object: ObjectRef<'gc>) {
        wrap_new_object(&mut self.no_gc, cx, self.args.this(), object);
        count(&self.caches.wrappers_made);
    }

    fn ret_object<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: ObjectRef<'gc>,
    ) -> Option<std::convert::Infallible> {
        if !self
            .no_gc
            .return_wrapper(&mut self.call.rv.op(), cx.object_wrapper_edge(object))
        {
            self.keep(Kept::Object(
                cx.root_object(object),
                dom_api::interface_of(cx, object),
            ));
        }
        None
    }

    fn construct_html<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, constructed: HtmlConstructed<'gc>) {
        let element = match constructed {
            HtmlConstructed::New(element) => element,
            HtmlConstructed::Upgrading(element) => {
                if self.no_gc.wrapper(cx.wrapper_edge(element)).is_some() {
                    self.keep(Kept::Upgrading(cx.root(element)));
                    return;
                }
                element
            }
        };
        wrap_new_object(
            &mut self.no_gc,
            cx,
            self.args.this(),
            ObjectRef::Node(element),
        );
        count(&self.caches.wrappers_made);
    }

    fn intersection_node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>) -> Option<NodeRef<'gc>> {
        let object = self.call.rare.as_ref()?.intersection_node?;
        self.no_gc.unwrap::<Node>(cx.gc, object)
    }

    fn static_range_nodes<'gc>(&mut self, cx: &ApiCx<'_, 'gc>) -> (NodeRef<'gc>, NodeRef<'gc>) {
        let [start, end] = self
            .call
            .rare
            .as_ref()
            .and_then(|rare| rare.static_range_nodes)
            .expect("the containers kept");
        let mut node = |object| {
            self.no_gc
                .unwrap::<Node>(cx.gc, object)
                .expect("a node's wrapper, checked")
        };
        (node(start), node(end))
    }

    fn listed_shadow_roots<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
    ) -> Result<Vec<NodeRef<'gc>>, ApiError> {
        let objects = self
            .call
            .rare
            .as_ref()
            .map(|rare| rare.shadow_roots.clone())
            .unwrap_or_default();
        objects
            .into_iter()
            .map(|object| {
                let node = self.no_gc.unwrap::<Node>(cx.gc, object).expect("a node's wrapper, checked as it was taken");
                if cx.is_shadow_root(node) {
                    Ok(node)
                } else {
                    Err(ApiError::type_error(
                        "Failed to execute 'getHTML': Failed to read the 'shadowRoots' property from 'GetHTMLOptions': Failed to convert value to 'ShadowRoot'.",
                    ))
                }
            })
            .collect()
    }

    fn new_target_number(&mut self) -> Option<u32> {
        v8::Local::<v8::Object>::try_from(self.args.new_target())
            .ok()
            .map(number)
    }

    fn is_new_target(&mut self, _: &ApiCx<'_, '_>, edge: &JsEdge) -> bool {
        let new_target = self.args.new_target();
        self.no_gc
            .wrapper(edge)
            .is_some_and(|held| v8::Local::<v8::Value>::from(held) == new_target)
    }

    fn callback_number(&mut self, callback: Callback<usize>) -> u32 {
        let _ = callback;
        let taken = self.call.callback.expect("the callback taken");
        v8::Local::<v8::Object>::try_from(taken).map_or(0, number)
    }

    fn ret_construct<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        definition: elise::Gc<'gc, dom::CustomElementDefinition>,
        document: NodeRef<'gc>,
    ) -> Option<std::convert::Infallible> {
        self.keep(Kept::Construct(
            Persistent::new(cx.gc, definition),
            cx.root(document),
        ));
        None
    }

    fn ret_any_or_undefined<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, edge: Option<&'gc JsEdge>) {
        if let Some(edge) = edge {
            self.ret_any(cx, edge);
        }
    }

    // V8's `undefined` is the return value until set.
    fn ret_undefined(&mut self) {}

    #[inline]
    fn ret_null(&mut self) {
        self.call.rv.op().set_null();
    }

    #[inline]
    fn ret_bool(&mut self, value: bool) {
        self.call.rv.op().set_bool(value);
    }

    /// A Smi's at once; past one a heap number, made after the operation.
    #[inline]
    fn ret_u32(&mut self, value: u32) -> Option<std::convert::Infallible> {
        if !self.call.rv.op().set_smi_u32(value) {
            self.keep(Kept::U32(value));
        }
        None
    }

    /// A Smi's at once; past one a heap number, made after the operation.
    #[inline]
    fn ret_i32(&mut self, value: i32) -> Option<std::convert::Infallible> {
        if !self.call.rv.op().set_smi_i32(value) {
            self.keep(Kept::F64(value.into()));
        }
        None
    }

    /// The value as it is (V8's handle), undefined if none.
    #[inline]
    fn ret_any<'gc>(&mut self, _: &ApiCx<'_, 'gc>, edge: &'gc JsEdge) {
        match self.no_gc.strong_value(edge) {
            Some(value) => self.call.rv.op().set(value),
            None => self.call.rv.op().set_undefined(),
        }
    }

    fn ret_any_or_null<'gc>(&mut self, _: &ApiCx<'_, 'gc>, edge: &'gc JsEdge) {
        match self.no_gc.strong_value(edge) {
            Some(value) => self.call.rv.op().set(value),
            None => self.call.rv.op().set_null(),
        }
    }

    /// A Smi's at once (an integer in its range, not -0); another a heap
    /// number, made after the operation.
    #[inline]
    fn ret_f64(&mut self, value: f64) -> Option<std::convert::Infallible> {
        let integer = value.fract() == 0.0
            && value.abs() <= f64::from(i32::MAX)
            && !(value == 0.0 && value.is_sign_negative());
        if !(integer
            && self
                .call
                .rv
                .op()
                .set_smi_i32(base::numerics::saturated_cast::<i32, f64>(value)))
        {
            self.keep(Kept::F64(value));
        }
        None
    }

    /// The string's cached V8 string, returned from the cache's weak handle
    /// (no local handle made, as Blink's `StringCache` returns its
    /// persistent); else kept, made after the operation.
    #[inline]
    fn ret_string(&mut self, value: &DomString) -> Option<std::convert::Infallible> {
        let returned = self
            .caches
            .strings
            .find(value)
            .is_some_and(|weak| self.no_gc.return_weak(&mut self.call.rv.op(), weak));
        if !returned {
            self.keep(Kept::String(value.clone()));
        }
        None
    }

    /// The name's V8 string, made once a DOM (Blink's `QualifiedName`s keep
    /// theirs in `StringCache` too), returned from its global with no
    /// local handle made, as V8's `ReturnValue::Set(const Global&)`.
    fn ret_name(&mut self, id: NameId) -> Option<std::convert::Infallible> {
        match usize::try_from(id)
            .ok()
            .and_then(|index| self.caches.names.get(index))
            .and_then(Option::as_ref)
        {
            Some(string) => self.no_gc.return_global(&mut self.call.rv.op(), string),
            None => self.keep(Kept::Name(id, names::string(id))),
        }
        None
    }

    /// The node's wrapper, written from the node's reference to it (no
    /// local handle made), if it has one.
    #[inline]
    fn ret_node<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        node: NodeRef<'gc>,
    ) -> Option<std::convert::Infallible> {
        if !self
            .no_gc
            .return_wrapper(&mut self.call.rv.op(), cx.wrapper_edge(node))
        {
            self.keep_node(cx, node);
        }
        None
    }

    #[inline]
    fn ret_new_node<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        node: NodeRef<'gc>,
        (): (),
    ) -> Option<std::convert::Infallible> {
        match self.call.prepared.take() {
            Some((wrapper, interface)) if interface == interface_of(cx, node) => {
                debug_assert!(
                    self.no_gc.wrapper(cx.wrapper_edge(node)).is_none(),
                    "a new node has no wrapper"
                );
                wrap_new(&mut self.no_gc, cx, wrapper, node);
                count(&self.caches.wrappers_made);
                count(&self.caches.wrappers_ahead);
                self.call.rv.op().set(wrapper.into());
                None
            }
            // Of another interface than the one foreseen: made after.
            prepared => {
                if prepared.is_some() {
                    count(&self.caches.wrappers_wasted);
                }
                self.ret_node(cx, node)
            }
        }
    }
}

/// A member of an `IntersectionObserverEntryInit`, converted.
#[derive(Clone, Copy)]
enum Init {
    Rect(Option<[f64; 4]>),
    Number(f64),
    Bool(bool),
}

/// How many frozen arrays a wrapper keeps (`frozen_array`): more than
/// any interface's `FrozenArray` attributes.
const FROZEN_ARRAYS: u32 = 8;

/// A `FrozenArray<T>` attribute's array (WebIDL's) of `values`: one
/// `holder` was given before if it has the same values (SameValue, in
/// order), so that reading twice gives the same array while nothing
/// changed (Blink's attributes keep their `FrozenArray`); a
/// new one frozen otherwise, kept with the wrapper's others (the last
/// `FROZEN_ARRAYS`). An attribute's values are its own (the thresholds, an
/// entry's sizes), so another's never match.
fn frozen_array<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    holder: v8::Local<'s, v8::Object>,
    values: &[v8::Local<'s, v8::Value>],
) -> v8::Local<'s, v8::Array> {
    let key = templates::string(scope, "machopo:frozen-arrays");
    let key = v8::Private::for_api(scope, Some(key));
    let kept = holder
        .get_private(scope, key)
        .and_then(|kept| v8::Local::<v8::Array>::try_from(kept).ok());
    let same = |scope: &mut v8::PinScope<'s, '_>, array: v8::Local<'s, v8::Array>| {
        u32::try_from(values.len()).is_ok_and(|length| length == array.length())
            && values.iter().enumerate().all(|(index, value)| {
                u32::try_from(index).is_ok_and(|index| {
                    array
                        .get_index(scope, index)
                        .is_some_and(|cached| cached.same_value(*value))
                })
            })
    };
    if let Some(kept) = kept {
        for index in 0..kept.length() {
            if let Some(array) = kept.get_index(scope, index)
                && let Ok(array) = v8::Local::<v8::Array>::try_from(array)
                && same(scope, array)
            {
                return array;
            }
        }
    }
    let array = v8::Array::new_with_elements(scope, values);
    array.set_integrity_level(scope, v8::IntegrityLevel::Frozen);
    // The kept ones, the oldest dropped past `FROZEN_ARRAYS`, and this one.
    let mut list: Vec<v8::Local<'_, v8::Value>> = Vec::new();
    if let Some(kept) = kept {
        let start = kept.length().saturating_sub(FROZEN_ARRAYS - 1);
        for index in start..kept.length() {
            if let Some(value) = kept.get_index(scope, index) {
                list.push(value);
            }
        }
    }
    list.push(array.into());
    let list = v8::Array::new_with_elements(scope, &list);
    holder.set_private(scope, key, list.into());
    array
}

/// One of `roundRect`'s radii (WebIDL's `(unrestricted double or
/// DOMPointInit)`): an object, null or undefined a `DOMPointInit` (its
/// members read in their names' order, `x` and `y` kept), anything else
/// `ToNumber`; `None` if it threw.
fn radius<'s>(scope: &mut v8::PinScope<'s, '_>, value: v8::Local<'s, v8::Value>) -> Option<Radius> {
    if !value.is_null_or_undefined() && !value.is_object() {
        return Some(Radius::Both(value.number_value(scope)?));
    }
    let mut point = [0.0, 0.0, 0.0, 1.0];
    if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
        // `w`, `x`, `y`, `z`: WebIDL reads a dictionary's members sorted.
        for (name, at) in [("w", 3), ("x", 0), ("y", 1), ("z", 2)] {
            let key = templates::string(scope, name);
            let member = object.get(scope, key.into())?;
            if !member.is_undefined() {
                point[at] = member.number_value(scope)?;
            }
        }
    }
    Some(Radius::Point {
        x: point[0],
        y: point[1],
    })
}
