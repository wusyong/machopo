//! The embedder's state of an isolate, in one of V8's isolate data slots
//! (Blink's `V8PerIsolateData`, in `gin::kEmbedderBlink`'s): read back with
//! a load (V8's `Isolate::GetData` is inline, and so is the fork's), not
//! rusty_v8's slot map, which hashes the state's `TypeId` on every look
//! (`Isolate::get_slot`), nor a thread-local.
//!
//! The state is borrowed only through its isolate, so that whoever holds a
//! borrow can't call V8 meanwhile: from the isolate (its owner, out of any
//! callback: `isolate_state_mut`), or from a scope of it (`StateScope`),
//! with what of V8 allocates nothing and calls nothing back
//! (`NoGc`). V8 reaches the state only from a call that took the isolate
//! whole (its GC calls the embedder heap as it allocates or steps:
//! `set_isolate_state_with_embedder_heap`), when no borrow of it is held.
//! A callback whose template was made where the state is a `T`
//! (`state_function_template`) borrows it with no check.

use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

use crate::unified_heap::EmbedderRegistration;

/// The isolate data slot (after rusty_v8's own) the state is in.
const STATE_SLOT: u32 = 0;

/// What the slot points to: the state's type, how to drop it, the embedder
/// heap's registration if the state is one, and the state, in the same box:
/// a look reads the type and the state from one line, with no load of the
/// state's address in between.
#[repr(C)]
struct Slot<T> {
    /// What `drop` and the cast to `Slot<T>` are for: at the same offset
    /// for any `T` (`repr(C)`, first), read through `Slot<()>`.
    type_id: TypeId,
    drop: unsafe fn(*mut Slot<()>),
    /// Dropped before the state, the slot still set: a GC V8 runs then is
    /// finished through it, with the state whole (`drop_isolate_state`).
    /// At the same offset for any `T`, as the two above.
    registration: Option<EmbedderRegistration>,
    value: T,
}

/// Gives `isolate` the embedder's state, once.
///
/// # Panics
///
/// If it has one already: a state is set once and dropped with the
/// isolate (`drop_isolate_state`), so that no borrow of it outlives it, and
/// a callback's template made where it was a `T` finds a `T`.
pub fn set_isolate_state<T: Any>(isolate: &mut v8::Isolate, state: T) {
    assert!(
        isolate.get_data(STATE_SLOT).is_null(),
        "an isolate's state is set once"
    );
    /// Drops the `Box<Slot<T>>` at `slot`.
    unsafe fn drop_box<T>(slot: *mut Slot<()>) {
        // SAFETY: as the caller promises: `Box::into_raw` of a
        // `Box<Slot<T>>`, dropped once.
        drop(unsafe { Box::from_raw(slot.cast::<Slot<T>>()) });
    }
    let slot = Box::new(Slot {
        type_id: TypeId::of::<T>(),
        drop: drop_box::<T>,
        registration: None,
        value: state,
    });
    isolate.set_data(STATE_SLOT, Box::into_raw(slot).cast());
}

/// Keeps `registration` with `isolate`'s state, dropped before it.
pub(crate) fn keep_registration(isolate: &mut v8::Isolate, registration: EmbedderRegistration) {
    let slot = isolate.get_data(STATE_SLOT).cast::<Slot<()>>();
    assert!(!slot.is_null(), "the state is set first");
    // SAFETY: what `set_isolate_state` put there, live until
    // `drop_isolate_state` empties the slot; its registration at the same
    // offset for any type; no borrow of it is held (`isolate` is borrowed
    // whole).
    unsafe { (*slot).registration = Some(registration) };
}

/// Whether `isolate` has an embedder state, of any type.
pub(crate) fn has_isolate_state(isolate: &v8::Isolate) -> bool {
    !isolate.get_data(STATE_SLOT).is_null()
}

/// The state's address, if it is a `T`.
#[inline]
fn checked<T: Any>(isolate: &v8::Isolate) -> Option<NonNull<T>> {
    let slot = NonNull::new(isolate.get_data(STATE_SLOT).cast::<Slot<()>>())?;
    // SAFETY: what `set_isolate_state` put there: a live `Slot` of some
    // type (only `drop_isolate_state` frees it, emptying the slot first),
    // read on the isolate's thread (`v8::Isolate` isn't `Sync`); its type
    // at the same offset for any (`repr(C)`, first).
    if unsafe { (*slot.as_ptr()).type_id } != TypeId::of::<T>() {
        return None;
    }
    let slot = slot.cast::<Slot<T>>();
    // SAFETY: a `Slot<T>`, as its type says; its value's address, not read
    // here.
    Some(unsafe { NonNull::new_unchecked(&raw mut (*slot.as_ptr()).value) })
}

/// The state's address, a `T`, with no check in a release build: one load
/// and an add.
///
/// # Safety
///
/// The isolate's state is a `T`.
#[inline]
pub(crate) unsafe fn unchecked<T: Any>(isolate: &v8::Isolate) -> NonNull<T> {
    debug_assert!(
        checked::<T>(isolate).is_some(),
        "the isolate's state is the callback's"
    );
    let slot = isolate.get_data(STATE_SLOT).cast::<Slot<T>>();
    // SAFETY: as the caller promises: a live `Slot<T>` (only
    // `drop_isolate_state` frees it, emptying the slot first); its value's
    // address, not read here.
    unsafe { NonNull::new_unchecked(&raw mut (*slot).value) }
}

/// The embedder's state of `isolate`, if it is a `T`: for the isolate's
/// owner, out of any callback, as long as it holds the isolate.
pub fn isolate_state_mut<T: Any>(isolate: &mut v8::Isolate) -> Option<&mut T> {
    // SAFETY: the state, alive while the isolate is (only
    // `drop_isolate_state` drops it, its contract no borrow after), not
    // reachable through the isolate, which is borrowed as long as it is:
    // nothing else borrows it, V8 included (it reaches the state only from
    // a call taking the isolate).
    checked::<T>(isolate).map(|mut state| unsafe { state.as_mut() })
}

/// Drops the embedder's state of `isolate`, if any: the embedder heap's
/// registration first (a GC V8 runs then finishes, reaching the state
/// whole), then the state.
///
/// # Safety
///
/// No callback of the isolate runs after (one whose template was made where
/// the state was a `T` would find none): the isolate's owner calls it as
/// the isolate goes.
pub unsafe fn drop_isolate_state(isolate: &mut v8::Isolate) {
    let slot = isolate.get_data(STATE_SLOT).cast::<Slot<()>>();
    if slot.is_null() {
        return;
    }
    // SAFETY: what `set_isolate_state` boxed, still in the slot; no borrow
    // of it is held (`isolate` is borrowed whole). Dropped out of the slot
    // first: its drop may run V8's GC, which reaches the state.
    let registration = unsafe { (*slot).registration.take() };
    drop(registration);
    isolate.set_data(STATE_SLOT, ptr::null_mut());
    // SAFETY: what `set_isolate_state` boxed, taken out of the slot just
    // now: dropped once, by its type's `drop` (read at the same offset for
    // any type); no borrow is used after, as the caller promises.
    unsafe { ((*slot).drop)(slot) };
}

/// What a holder of the state may still ask of V8: what allocates nothing
/// in its heap and calls nothing back, so that V8's GC (which reaches the
/// state) can't run meanwhile. Holds the scope as long as the state is
/// held; made only with a borrow of it (`StateScope`).
pub struct NoGc<'b, 's, 'i> {
    pub(crate) scope: &'b mut v8::PinScope<'s, 'i>,
}

/// A call's receiver that V8 checked is an object of one of the isolate's
/// templates, or a global proxy whose global is one (V8's
/// `IsCompatibleReceiver`): either way an object whose embedder field
/// follows its header, which `unified_heap::unwrap_receiver` reads without
/// its map (V8's header's `Object::Unwrap` doesn't read it either). Made
/// only here: by the
/// callbacks of the functions this module gives a `Signature`
/// (`state_method_template`, `state_cached_getter_template`: V8 calls them
/// only past it), and by the indexed interceptors it installs on an object
/// template (V8 calls them with the object the interceptor is on, one of
/// the template's, an API wrapper of its own type, as an object with an
/// interceptor is).
#[derive(Clone, Copy)]
pub struct Receiver<'s>(v8::Local<'s, v8::Object>);

impl<'s> Receiver<'s> {
    pub fn object(self) -> v8::Local<'s, v8::Object> {
        self.0
    }
}

/// A callback given its isolate's state, a `T`, and its receiver, which V8
/// checked against the function's signature: what `state_method_template`
/// and `state_cached_getter_template` call.
pub trait StateMethod<T>: 'static {
    fn call<'s>(
        scope: StateScope<'_, 's, '_, T>,
        args: v8::FunctionCallbackArguments<'s>,
        this: Receiver<'s>,
        rv: v8::ReturnValue<'s>,
    );
}

/// A callback given its isolate's state, a `T`: what
/// `state_function_template` calls.
pub trait StateCallback<T>: 'static {
    fn call<'s>(
        scope: StateScope<'_, 's, '_, T>,
        args: v8::FunctionCallbackArguments<'s>,
        rv: v8::ReturnValue<'s>,
    );
}

/// A method given V8's callback info alone, its isolate's state a `T`
/// (`StateInfo`): what `state_info_method_template` calls. No scope is
/// made for it: it makes one where it needs one (`StateInfo::with_scope`).
pub trait StateInfoMethod<T>: 'static {
    fn call(info: StateInfo<'_, T>);
}

/// An indexed getter given V8's callback info alone (`StateIndexInfo`):
/// what `set_state_info_indexed_getter` calls for `object[index]`.
pub trait StateInfoIndexedGetter<T>: 'static {
    fn get(info: StateIndexInfo<'_, T>) -> v8::Intercepted;
}

/// A named getter given V8's callback info alone (`StateNameInfo`): what
/// `info_named_getter`'s callback calls for `object.name`.
pub trait StateInfoNamedGetter<T>: 'static {
    fn get(info: StateNameInfo<'_, T>) -> v8::Intercepted;
}

/// A named setter given V8's callback info alone: `object.name = value`.
pub trait StateInfoNamedSetter<T>: 'static {
    fn set(info: StateNameInfo<'_, T>) -> v8::Intercepted;
}

/// A function given V8's callback info alone, as `StateInfoMethod`, its
/// receiver unchecked (a static operation's: it has no signature): what
/// `state_info_function_template` calls.
pub trait StateInfoFunction<T>: 'static {
    fn call(info: StateInfo<'_, T, Unsigned>);
}

/// An indexed getter given its isolate's state, a `T`: what
/// `set_state_indexed_getter` calls, and the queries of its properties.
pub trait StateIndexedGetter<T>: 'static {
    /// `this` the interceptor's holder.
    fn get<'s>(
        scope: StateScope<'_, 's, '_, T>,
        index: u32,
        args: v8::PropertyCallbackArguments<'s>,
        this: Receiver<'s>,
        rv: v8::ReturnValue<'_, v8::Value>,
    ) -> v8::Intercepted;

    /// The property's attributes (`PropertyAttribute`'s bits), if there
    /// is one at `index`.
    fn query<'s>(
        scope: StateScope<'_, 's, '_, T>,
        index: u32,
        args: v8::PropertyCallbackArguments<'s>,
        rv: v8::ReturnValue<'_, v8::Integer>,
    ) -> v8::Intercepted;

    /// The indices there are properties at.
    fn enumerate<'s>(
        scope: StateScope<'_, 's, '_, T>,
        args: v8::PropertyCallbackArguments<'s>,
        rv: v8::ReturnValue<'_, v8::Array>,
    );
}

/// An indexed setter given its isolate's state, a `T`: what
/// `set_state_indexed_properties` calls with the handler's data.
pub trait StateIndexedSetter<T>: 'static {
    fn set<'s>(
        scope: StateScope<'_, 's, '_, T>,
        index: u32,
        value: v8::Local<'s, v8::Value>,
        args: v8::PropertyCallbackArguments<'s>,
    ) -> v8::Intercepted;
}

/// A property an interceptor is asked of: a string name, or an array
/// index (which V8 sends to the indexed interceptor).
#[derive(Clone, Copy)]
pub enum PropertyKey<'s> {
    Name(v8::Local<'s, v8::Name>),
    Index(u32),
}

/// Named properties given their isolate's state, a `T`: what
/// `set_state_named_properties` calls, for string names and array indices
/// alike (WebIDL's named properties are by any string).
pub trait StateNamedProperties<T>: 'static {
    fn get<'s>(
        scope: StateScope<'_, 's, '_, T>,
        key: PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        rv: v8::ReturnValue<'_, v8::Value>,
    ) -> v8::Intercepted;

    /// The property's attributes (`PropertyAttribute`'s bits), if there
    /// is one.
    fn query<'s>(
        scope: StateScope<'_, 's, '_, T>,
        key: PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        rv: v8::ReturnValue<'_, v8::Integer>,
    ) -> v8::Intercepted;

    fn set<'s>(
        scope: StateScope<'_, 's, '_, T>,
        key: PropertyKey<'s>,
        value: v8::Local<'s, v8::Value>,
        args: v8::PropertyCallbackArguments<'s>,
    ) -> v8::Intercepted;

    fn define<'s>(
        scope: StateScope<'_, 's, '_, T>,
        key: PropertyKey<'s>,
        descriptor: &v8::PropertyDescriptor,
        args: v8::PropertyCallbackArguments<'s>,
        rv: v8::ReturnValue<'_, v8::Boolean>,
    ) -> v8::Intercepted;

    fn delete<'s>(
        scope: StateScope<'_, 's, '_, T>,
        key: PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        rv: v8::ReturnValue<'_, v8::Boolean>,
    ) -> v8::Intercepted;

    fn enumerate<'s>(
        scope: StateScope<'_, 's, '_, T>,
        args: v8::PropertyCallbackArguments<'s>,
        rv: v8::ReturnValue<'_, v8::Array>,
    );
}

/// What of `StateNamedProperties` an interface has.
#[derive(Clone, Copy)]
pub struct NamedOptions {
    /// Whether its names shadow the object's and its prototypes' own
    /// properties (WebIDL's `[LegacyOverrideBuiltIns]`): a masking
    /// interceptor; else a non-masking one, asked only when nothing else
    /// has the name.
    pub masking: bool,
    pub setter: bool,
    pub deleter: bool,
    pub enumerator: bool,
    /// Whether array indices are names too (`dataset[9]`); else the
    /// interface has indexed properties of its own, its indexed handler
    /// left as it is.
    pub indices: bool,
    /// The getter of a string name, if it is one given V8's callback
    /// info alone (`info_named_getter`) in `StateNamedProperties::get`'s
    /// place; and the setter (`info_named_setter`), in `set`'s. An array
    /// index as a name, and the other callbacks, are
    /// `StateNamedProperties`' either way.
    pub getter_info: Option<v8::NamedPropertyGetterCallback>,
    pub setter_info: Option<v8::NamedPropertySetterCallback>,
}

/// A scope whose isolate's state is a `T` (a state callback's, its template
/// made where it is one; or checked, `new`), which a state is from then on
/// (set once, dropped with the isolate): borrowed with no check.
pub struct StateScope<'a, 's, 'i, T> {
    scope: &'a mut v8::PinScope<'s, 'i>,
    _state: PhantomData<fn() -> T>,
}

impl<'a, 's, 'i, T: Any> StateScope<'a, 's, 'i, T> {
    /// `scope`, if its isolate's state is a `T` (checked once): for what
    /// isn't a state callback (another callback, the isolate's owner).
    pub fn new(scope: &'a mut v8::PinScope<'s, 'i>) -> Option<Self> {
        checked::<T>(scope)?;
        Some(Self {
            scope,
            _state: PhantomData,
        })
    }

    /// The scope, all of V8's: the state isn't borrowed meanwhile.
    #[inline]
    pub fn scope(&mut self) -> &mut v8::PinScope<'s, 'i> {
        self.scope
    }

    /// The state, shared, and what of V8 its holder may use.
    #[inline]
    pub fn state(&mut self) -> (&T, NoGc<'_, 's, 'i>) {
        // SAFETY: the state is a `T` (`StateScope`'s); as
        // `isolate_state_mut`'s, the scope borrowed for as long and handed
        // to the `NoGc` alone.
        let state = unsafe { unchecked::<T>(self.scope).as_ref() };
        (state, NoGc { scope: self.scope })
    }

    /// The state, and what of V8 its holder may use.
    #[inline]
    pub fn state_mut(&mut self) -> (&mut T, NoGc<'_, 's, 'i>) {
        // SAFETY: as `state`'s.
        let state = unsafe { unchecked::<T>(self.scope).as_mut() };
        (state, NoGc { scope: self.scope })
    }
}

/// A function callback's info (V8's `FunctionCallbackInfo`: the isolate,
/// the receiver, the arguments and where the result goes are all read from
/// it), its isolate's state a `T`: what Blink's generated callbacks take.
/// rusty_v8's `CallbackScope` is its own record of the isolate, not V8's
/// (V8 opens the callback's handle scope itself): a callback that reads
/// the state, unwraps, and returns what has a handle already needs none,
/// and one that allocates, throws or runs script makes one then
/// (`with_scope`). One a callback, its trampoline's: the state is borrowed
/// through it alone.
///
/// `K` tells what V8 checked before the call: `Signed`, the receiver
/// against the function's signature (a method's, `receiver`); `Unsigned`,
/// nothing (a static operation's, whose `this` is anything).
pub struct StateInfo<'s, T, K = Signed> {
    info: &'s v8::FunctionCallbackInfo,
    _state: PhantomData<fn() -> (T, K)>,
}

/// What a callback given V8's callback info alone reads through it,
/// whichever kind of callback it is (a function's `StateInfo`, an indexed
/// getter's `StateIndexInfo`): the isolate, the state with what of V8 its
/// holder may use, and a scope where it needs one.
pub trait InfoState<'s, T> {
    /// The isolate, no scope made.
    fn isolate(&mut self) -> &mut v8::Isolate;
    /// The state, shared, and what of V8 its holder may use.
    fn state(&mut self) -> (&T, InfoNoGc<'_, 's>);
    /// The state, and what of V8 its holder may use.
    fn state_mut(&mut self) -> (&mut T, InfoNoGc<'_, 's>);
    /// `f` of a scope made now and the call's result.
    fn with_scope<R>(
        &mut self,
        f: impl FnOnce(StateScope<'_, 's, 's, T>, v8::ReturnValue<'s, v8::Value>) -> R,
    ) -> R;
}

impl<'s, T: Any, K> InfoState<'s, T> for StateInfo<'s, T, K> {
    #[inline(always)]
    fn isolate(&mut self) -> &mut v8::Isolate {
        StateInfo::isolate(self)
    }

    #[inline(always)]
    fn state(&mut self) -> (&T, InfoNoGc<'_, 's>) {
        StateInfo::state(self)
    }

    #[inline(always)]
    fn state_mut(&mut self) -> (&mut T, InfoNoGc<'_, 's>) {
        StateInfo::state_mut(self)
    }

    #[inline(always)]
    fn with_scope<R>(
        &mut self,
        f: impl FnOnce(StateScope<'_, 's, 's, T>, v8::ReturnValue<'s, v8::Value>) -> R,
    ) -> R {
        StateInfo::with_scope(self, f)
    }
}

/// An indexed getter's interceptor's info (V8's `PropertyCallbackInfo`)
/// and its index, its isolate's state a `T`: as `StateInfo` is a
/// function's, with no scope made (Blink's generated
/// `IndexedPropertyGetterCallback` takes the same). One a callback, its
/// trampoline's.
pub struct StateIndexInfo<'s, T> {
    info: &'s v8::PropertyCallbackInfo<v8::Value>,
    /// The isolate's pointer, read from the info once.
    isolate: v8::Isolate,
    index: u32,
    _state: PhantomData<fn() -> T>,
}

impl<'s, T: Any> StateIndexInfo<'s, T> {
    /// The index asked for.
    #[inline(always)]
    pub fn index(&self) -> u32 {
        self.index
    }

    /// The object the interceptor is on: one of the template's (V8 calls
    /// an interceptor with it), as `indexed_trampoline`'s.
    #[inline(always)]
    pub fn holder(&self) -> Receiver<'s> {
        Receiver(self.info.arguments().holder())
    }
}

impl<'s, T: Any> InfoState<'s, T> for StateIndexInfo<'s, T> {
    #[inline(always)]
    fn isolate(&mut self) -> &mut v8::Isolate {
        &mut self.isolate
    }

    #[inline(always)]
    fn state(&mut self) -> (&T, InfoNoGc<'_, 's>) {
        let rv = v8::ReturnValue::from_property_callback_info(self.info);
        let isolate = &mut self.isolate;
        // SAFETY: as `StateInfo::state`'s: the state is a `T` (the
        // interceptor was set where it is one), the isolate borrowed for
        // as long and handed to the `InfoNoGc` alone.
        let state = unsafe { unchecked::<T>(isolate).as_ref() };
        (state, InfoNoGc { isolate, rv })
    }

    #[inline(always)]
    fn state_mut(&mut self) -> (&mut T, InfoNoGc<'_, 's>) {
        let rv = v8::ReturnValue::from_property_callback_info(self.info);
        let isolate = &mut self.isolate;
        // SAFETY: as `state`'s.
        let state = unsafe { unchecked::<T>(isolate).as_mut() };
        (state, InfoNoGc { isolate, rv })
    }

    #[inline(never)]
    fn with_scope<R>(
        &mut self,
        f: impl FnOnce(StateScope<'_, 's, 's, T>, v8::ReturnValue<'s, v8::Value>) -> R,
    ) -> R {
        if cfg!(debug_assertions) {
            INFO_SCOPES.set(INFO_SCOPES.get().wrapping_add(1));
        }
        // SAFETY: as `StateInfo::with_scope`'s, of an interceptor's info
        // (rusty_v8's own trampoline makes its scope of it).
        v8::callback_scope!(unsafe scope, self.info);
        f(
            StateScope {
                scope,
                _state: PhantomData,
            },
            v8::ReturnValue::from_property_callback_info(self.info),
        )
    }
}

/// A named property's interceptor's info (V8's `PropertyCallbackInfo`),
/// the name asked for and (a setter's) the value, its isolate's state a
/// `T`: as `StateIndexInfo` is an indexed getter's. A setter's result
/// isn't the info's to give (V8 reads what the callback returns): its
/// holder writes none.
pub struct StateNameInfo<'s, T> {
    info: &'s v8::PropertyCallbackInfo<v8::Value>,
    isolate: v8::Isolate,
    name: v8::Local<'s, v8::Name>,
    value: Option<v8::Local<'s, v8::Value>>,
    _state: PhantomData<fn() -> T>,
}

impl<'s, T: Any> StateNameInfo<'s, T> {
    /// The name asked for: a string (the interceptor takes no symbols).
    #[inline(always)]
    pub fn name(&self) -> v8::Local<'s, v8::Name> {
        self.name
    }

    /// The value a setter is given; none for a getter.
    #[inline(always)]
    pub fn value(&self) -> Option<v8::Local<'s, v8::Value>> {
        self.value
    }

    /// The object the interceptor is on, as `StateIndexInfo::holder`.
    #[inline(always)]
    pub fn holder(&self) -> Receiver<'s> {
        Receiver(self.info.arguments().holder())
    }
}

impl<'s, T: Any> InfoState<'s, T> for StateNameInfo<'s, T> {
    #[inline(always)]
    fn isolate(&mut self) -> &mut v8::Isolate {
        &mut self.isolate
    }

    #[inline(always)]
    fn state(&mut self) -> (&T, InfoNoGc<'_, 's>) {
        let rv = v8::ReturnValue::from_property_callback_info(self.info);
        let isolate = &mut self.isolate;
        // SAFETY: as `StateIndexInfo::state`'s.
        let state = unsafe { unchecked::<T>(isolate).as_ref() };
        (state, InfoNoGc { isolate, rv })
    }

    #[inline(always)]
    fn state_mut(&mut self) -> (&mut T, InfoNoGc<'_, 's>) {
        let rv = v8::ReturnValue::from_property_callback_info(self.info);
        let isolate = &mut self.isolate;
        // SAFETY: as `state`'s.
        let state = unsafe { unchecked::<T>(isolate).as_mut() };
        (state, InfoNoGc { isolate, rv })
    }

    #[inline(never)]
    fn with_scope<R>(
        &mut self,
        f: impl FnOnce(StateScope<'_, 's, 's, T>, v8::ReturnValue<'s, v8::Value>) -> R,
    ) -> R {
        if cfg!(debug_assertions) {
            INFO_SCOPES.set(INFO_SCOPES.get().wrapping_add(1));
        }
        // SAFETY: as `StateIndexInfo::with_scope`'s.
        v8::callback_scope!(unsafe scope, self.info);
        f(
            StateScope {
                scope,
                _state: PhantomData,
            },
            v8::ReturnValue::from_property_callback_info(self.info),
        )
    }
}

/// A `StateInfo` of a function with a signature: its receiver is one of
/// the isolate's templates' objects (`StateInfo::receiver`).
pub struct Signed;

/// A `StateInfo` of a function with no signature: no `Receiver` is made
/// of its `this`.
pub struct Unsigned;

/// What a holder of the state borrowed from a `StateInfo` may ask of V8:
/// `NoGc`'s reads that need the isolate alone, and the callback's result
/// (`OpReturn`). Holds the isolate as long as the state is held.
pub struct InfoNoGc<'b, 's> {
    pub(crate) isolate: &'b mut v8::Isolate,
    pub(crate) rv: v8::ReturnValue<'s, v8::Value>,
}

thread_local! {
    /// The scopes the info callbacks made on this thread
    /// (`StateInfo::with_scope`), for the tests. Counted in debug builds
    /// only.
    static INFO_SCOPES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many scopes this thread's info callbacks made
/// (`StateInfo::with_scope`): what tells a member's call needed none.
/// `None` in release builds. For tests.
pub fn info_scopes_for_testing() -> Option<u64> {
    cfg!(debug_assertions).then(|| INFO_SCOPES.get())
}

impl<'s, T: Any> StateInfo<'s, T, Signed> {
    /// The receiver, which V8 checked against the function's signature
    /// (`state_info_method_template`).
    #[inline(always)]
    pub fn receiver(&self) -> Receiver<'s> {
        Receiver(self.args().this())
    }
}

impl<'s, T: Any, K> StateInfo<'s, T, K> {
    /// The isolate, no scope made: for what reads V8's objects in place.
    #[inline(always)]
    pub fn isolate(&mut self) -> &mut v8::Isolate {
        // SAFETY: the slot of the callback's frame that holds the
        // isolate's pointer, which an `Isolate` is; valid while the
        // callback runs (`'s`). Borrowed as `self` is, and `self` is the
        // callback's only way to it (a scope made of the info keeps a
        // copy of the pointer, elsewhere).
        unsafe { &mut *self.info.isolate().as_ptr() }
    }

    /// The call's arguments: reading them allocates nothing.
    #[inline(always)]
    pub fn args(&self) -> v8::FunctionCallbackArguments<'s> {
        v8::FunctionCallbackArguments::from_function_callback_info(self.info)
    }

    /// The state, shared, and what of V8 its holder may use.
    #[inline(always)]
    pub fn state(&mut self) -> (&T, InfoNoGc<'_, 's>) {
        let rv = v8::ReturnValue::from_function_callback_info(self.info);
        let isolate = self.isolate();
        // SAFETY: the state is a `T` (the template was made where it is
        // one); as `StateScope::state`'s, the isolate borrowed for as long
        // and handed to the `InfoNoGc` alone.
        let state = unsafe { unchecked::<T>(isolate).as_ref() };
        (state, InfoNoGc { isolate, rv })
    }

    /// The state, and what of V8 its holder may use.
    #[inline(always)]
    pub fn state_mut(&mut self) -> (&mut T, InfoNoGc<'_, 's>) {
        let rv = v8::ReturnValue::from_function_callback_info(self.info);
        let isolate = self.isolate();
        // SAFETY: as `state`'s.
        let state = unsafe { unchecked::<T>(isolate).as_mut() };
        (state, InfoNoGc { isolate, rv })
    }

    /// `f` of a scope made now and the call's result, the scope dropped
    /// as `f` returns: all of V8's, for what allocates, throws or runs
    /// script (the state isn't borrowed meanwhile). What rusty_v8's own
    /// trampoline gives every callback. Out of line: the calls that need
    /// one are the slow ones.
    #[inline(never)]
    pub fn with_scope<R>(
        &mut self,
        f: impl FnOnce(StateScope<'_, 's, 's, T>, v8::ReturnValue<'s, v8::Value>) -> R,
    ) -> R {
        if cfg!(debug_assertions) {
            INFO_SCOPES.set(INFO_SCOPES.get().wrapping_add(1));
        }
        // SAFETY: V8's callback info, alive for `'s`, as rusty_v8's
        // trampoline makes a scope of; the handles made in it are of V8's
        // own handle scope of the callback, so they outlive this scope
        // (rusty_v8's `CallbackScope` gives them the info's lifetime).
        v8::callback_scope!(unsafe scope, self.info);
        f(
            StateScope {
                scope,
                _state: PhantomData,
            },
            v8::ReturnValue::from_function_callback_info(self.info),
        )
    }
}

/// An info method's callback: V8's own (`FunctionTemplate::builder_raw`),
/// given the info alone.
unsafe extern "C" fn info_method_trampoline<T: Any, C: StateInfoMethod<T>>(
    info: *const v8::FunctionCallbackInfo,
) {
    C::call(StateInfo {
        // SAFETY: V8's callback info, valid for the call.
        info: unsafe { &*info },
        _state: PhantomData,
    });
}

/// An info function's callback, as `info_method_trampoline`.
unsafe extern "C" fn info_function_trampoline<T: Any, C: StateInfoFunction<T>>(
    info: *const v8::FunctionCallbackInfo,
) {
    C::call(StateInfo {
        // SAFETY: V8's callback info, valid for the call.
        info: unsafe { &*info },
        _state: PhantomData,
    });
}

/// An info indexed getter's callback: V8's own (`getter_raw`), given the
/// index and the info alone.
unsafe extern "C" fn info_indexed_trampoline<T: Any, G: StateInfoIndexedGetter<T>>(
    index: u32,
    info: *const v8::PropertyCallbackInfo<v8::Value>,
) -> v8::Intercepted {
    // SAFETY: V8's callback info, valid for the call.
    let info = unsafe { &*info };
    G::get(StateIndexInfo {
        info,
        isolate: info.isolate(),
        index,
        _state: PhantomData,
    })
}

/// An info named getter's callback: V8's own (`getter_raw`), given the
/// name and the info alone.
unsafe extern "C" fn info_named_get_trampoline<T: Any, G: StateInfoNamedGetter<T>>(
    name: v8::SealedLocal<v8::Name>,
    info: *const v8::PropertyCallbackInfo<v8::Value>,
) -> v8::Intercepted {
    // SAFETY: V8's callback info, valid for the call.
    let info = unsafe { &*info };
    G::get(StateNameInfo {
        info,
        isolate: info.isolate(),
        // SAFETY: the name V8 passed the callback, used within it.
        name: unsafe { name.unseal_in_callback() },
        value: None,
        _state: PhantomData,
    })
}

/// An info named setter's callback, as `info_named_get_trampoline`.
unsafe extern "C" fn info_named_set_trampoline<T: Any, S: StateInfoNamedSetter<T>>(
    name: v8::SealedLocal<v8::Name>,
    value: v8::SealedLocal<v8::Value>,
    info: *const v8::PropertyCallbackInfo<v8::Boolean>,
) -> v8::Intercepted {
    // SAFETY: V8's callback info, valid for the call. Its return value's
    // type is a phantom of rusty_v8's (`PropertyCallbackInfo` is
    // `repr(C)` of the raw info and a `PhantomData`), and a setter's is
    // never written (`StateNameInfo`).
    let info = unsafe { &*info.cast::<v8::PropertyCallbackInfo<v8::Value>>() };
    S::set(StateNameInfo {
        info,
        isolate: info.isolate(),
        // SAFETY: the name and the value V8 passed the callback, used
        // within it.
        name: unsafe { name.unseal_in_callback() },
        value: Some(unsafe { value.unseal_in_callback() }),
        _state: PhantomData,
    })
}

/// The callback of `G`'s named getter given V8's callback info alone,
/// for `NamedOptions::getter_info`.
pub fn info_named_getter<T: Any, G: StateInfoNamedGetter<T>>() -> v8::NamedPropertyGetterCallback {
    info_named_get_trampoline::<T, G>
}

/// As `info_named_getter`, a setter's (`NamedOptions::setter_info`).
pub fn info_named_setter<T: Any, S: StateInfoNamedSetter<T>>() -> v8::NamedPropertySetterCallback {
    info_named_set_trampoline::<T, S>
}

/// The callback of `C`, given the state.
fn function_trampoline<'s, T: Any, C: StateCallback<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    rv: v8::ReturnValue<'s>,
) {
    C::call(
        StateScope {
            scope,
            _state: PhantomData,
        },
        args,
        rv,
    );
}

/// A method's callback, given the state: the function's signature V8
/// checked the receiver against (`state_method_template`).
fn method_trampoline<'s, T: Any, C: StateMethod<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    rv: v8::ReturnValue<'s>,
) {
    let this = Receiver(args.this());
    C::call(
        StateScope {
            scope,
            _state: PhantomData,
        },
        args,
        this,
        rv,
    );
}

/// The indexed getter of `G`, given the state: its holder the object the
/// interceptor is on (`Receiver`).
fn indexed_trampoline<'s, T: Any, G: StateIndexedGetter<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Value>,
) -> v8::Intercepted {
    let this = Receiver(args.holder());
    G::get(
        StateScope {
            scope,
            _state: PhantomData,
        },
        index,
        args,
        this,
        rv,
    )
}

fn indexed_query<'s, T: Any, G: StateIndexedGetter<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Integer>,
) -> v8::Intercepted {
    G::query(state_scope(scope), index, args, rv)
}

fn indexed_enumerate<'s, T: Any, G: StateIndexedGetter<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Array>,
) {
    G::enumerate(state_scope(scope), args, rv);
}

fn state_scope<'a, 's, 'i, T>(scope: &'a mut v8::PinScope<'s, 'i>) -> StateScope<'a, 's, 'i, T> {
    StateScope {
        scope,
        _state: PhantomData,
    }
}

fn named_get<'s, T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Value>,
) -> v8::Intercepted {
    N::get(state_scope(scope), PropertyKey::Name(name), args, rv)
}

fn named_query<'s, T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Integer>,
) -> v8::Intercepted {
    N::query(state_scope(scope), PropertyKey::Name(name), args, rv)
}

fn named_set<'s, T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    value: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    _: v8::ReturnValue<'_, v8::Boolean>,
) -> v8::Intercepted {
    N::set(state_scope(scope), PropertyKey::Name(name), value, args)
}

fn named_define<'s, T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    descriptor: &v8::PropertyDescriptor,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Boolean>,
) -> v8::Intercepted {
    N::define(
        state_scope(scope),
        PropertyKey::Name(name),
        descriptor,
        args,
        rv,
    )
}

fn named_delete<'s, T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Boolean>,
) -> v8::Intercepted {
    N::delete(state_scope(scope), PropertyKey::Name(name), args, rv)
}

fn named_enumerate<'s, T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Array>,
) {
    N::enumerate(state_scope(scope), args, rv);
}

fn index_get<'s, T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Value>,
) -> v8::Intercepted {
    N::get(state_scope(scope), PropertyKey::Index(index), args, rv)
}

fn index_query<'s, T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Integer>,
) -> v8::Intercepted {
    N::query(state_scope(scope), PropertyKey::Index(index), args, rv)
}

fn index_set<'s, T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    value: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    _: v8::ReturnValue<'_, v8::Boolean>,
) -> v8::Intercepted {
    N::set(state_scope(scope), PropertyKey::Index(index), value, args)
}

fn index_define<'s, T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    descriptor: &v8::PropertyDescriptor,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Boolean>,
) -> v8::Intercepted {
    N::define(
        state_scope(scope),
        PropertyKey::Index(index),
        descriptor,
        args,
        rv,
    )
}

fn index_delete<'s, T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Boolean>,
) -> v8::Intercepted {
    N::delete(state_scope(scope), PropertyKey::Index(index), args, rv)
}

/// A function template whose callback is `C`'s, given the isolate's state:
/// made only where the state is a `T`, and built here, as `configure` has
/// it, so that the callback hangs on no other isolate's template.
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn state_function_template<'s, T: Any, C: StateCallback<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    configure: impl FnOnce(
        v8::FunctionBuilder<'s, v8::FunctionTemplate>,
    ) -> v8::FunctionBuilder<'s, v8::FunctionTemplate>,
) -> v8::Local<'s, v8::FunctionTemplate> {
    assert!(
        checked::<T>(scope).is_some(),
        "a state callback's template is made where the state is one"
    );
    configure(v8::FunctionTemplate::builder(function_trampoline::<T, C>)).build(scope)
}

/// A function template whose callback is `C`'s, given the isolate's state
/// and its receiver (`Receiver`), which V8 checks against `signature`
/// before each call. `configure` sets the rest; the signature is set last,
/// so that it is the function's.
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn state_method_template<'s, T: Any, C: StateMethod<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    signature: v8::Local<'s, v8::Signature>,
    configure: impl FnOnce(
        v8::FunctionBuilder<'s, v8::FunctionTemplate>,
    ) -> v8::FunctionBuilder<'s, v8::FunctionTemplate>,
) -> v8::Local<'s, v8::FunctionTemplate> {
    assert!(
        checked::<T>(scope).is_some(),
        "a state callback's template is made where the state is one"
    );
    configure(v8::FunctionTemplate::builder(method_trampoline::<T, C>))
        .signature(signature)
        .build(scope)
}

/// A function template whose callback is `C`'s, given V8's callback info
/// alone (`StateInfo`), its receiver checked against `signature` before
/// each call. `configure` sets the rest; the signature is set last, so
/// that it is the function's.
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn state_info_method_template<'s, T: Any, C: StateInfoMethod<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    signature: v8::Local<'s, v8::Signature>,
    configure: impl FnOnce(
        v8::FunctionBuilder<'s, v8::FunctionTemplate>,
    ) -> v8::FunctionBuilder<'s, v8::FunctionTemplate>,
) -> v8::Local<'s, v8::FunctionTemplate> {
    assert!(
        checked::<T>(scope).is_some(),
        "a state callback's template is made where the state is one"
    );
    configure(v8::FunctionTemplate::builder_raw(
        info_method_trampoline::<T, C>,
    ))
    .signature(signature)
    .build(scope)
}

/// A function template whose callback is `C`'s, given V8's callback info
/// alone, with no signature: its `this` is anything (a static operation's,
/// on an interface object). `configure` sets the rest.
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn state_info_function_template<'s, T: Any, C: StateInfoFunction<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    configure: impl FnOnce(
        v8::FunctionBuilder<'s, v8::FunctionTemplate>,
    ) -> v8::FunctionBuilder<'s, v8::FunctionTemplate>,
) -> v8::Local<'s, v8::FunctionTemplate> {
    assert!(
        checked::<T>(scope).is_some(),
        "a state callback's template is made where the state is one"
    );
    configure(v8::FunctionTemplate::builder_raw(
        info_function_trampoline::<T, C>,
    ))
    .build(scope)
}

/// A getter's function template whose callback is `C`'s, given the
/// isolate's state and its receiver, which V8 checks against `signature`
/// (as `state_method_template`'s), whose result V8 reads
/// from `cache`, a private property of the object the accessor is found
/// on, once that has one (Blink's `[CachedAccessor]`): V8's inline caches
/// and optimizing compilers load it as data. The embedder sets the
/// property, and sets it again when the value changes. Not a constructor.
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn state_cached_getter_template<'s, T: Any, C: StateMethod<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    cache: v8::Local<'s, v8::Private>,
    signature: v8::Local<'s, v8::Signature>,
) -> v8::Local<'s, v8::FunctionTemplate> {
    assert!(
        checked::<T>(scope).is_some(),
        "a state callback's template is made where the state is one"
    );
    let template = v8::machopo::function_template_with_cache(
        scope,
        method_trampoline::<T, C>,
        cache,
        Some(signature),
        0,
        v8::SideEffectType::HasSideEffect,
    );
    // What `ConstructorBehavior::Throw` does.
    template.remove_prototype();
    template
}

/// Makes `template`'s (its isolate's) objects callable, calling `C`'s
/// callback given the isolate's state, its `this` the object called (V8's
/// for an object template's call handler).
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn set_state_call_as_function_handler<T: Any, C: StateCallback<T>>(
    scope: &mut v8::PinScope<'_, '_>,
    template: v8::Local<'_, v8::ObjectTemplate>,
) {
    assert!(
        checked::<T>(scope).is_some(),
        "a state callback's template is made where the state is one"
    );
    template.set_call_as_function_handler(function_trampoline::<T, C>, None);
}

/// Gives `template` (its isolate's) the indexed getter of `G`, given the
/// isolate's state, each callback given `data`
/// (`PropertyCallbackArguments::data`).
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn set_state_indexed_getter<T: Any, G: StateIndexedGetter<T>>(
    scope: &mut v8::PinScope<'_, '_>,
    template: v8::Local<'_, v8::ObjectTemplate>,
    data: v8::Local<'_, v8::Value>,
) {
    assert!(
        checked::<T>(scope).is_some(),
        "a state callback's template is made where the state is one"
    );
    template.set_indexed_property_handler(
        v8::IndexedPropertyHandlerConfiguration::new()
            .getter(indexed_trampoline::<T, G>)
            .query(indexed_query::<T, G>)
            .enumerator(indexed_enumerate::<T, G>)
            .data(data),
    );
}

/// As `set_state_indexed_getter`, the getter itself given V8's callback
/// info alone (`StateInfoIndexedGetter`); with a setter of `S` if
/// `setter`. The query and the enumerator are as before.
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn set_state_info_indexed_getter<
    T: Any,
    G: StateInfoIndexedGetter<T> + StateIndexedGetter<T>,
    S: StateIndexedSetter<T>,
>(
    scope: &mut v8::PinScope<'_, '_>,
    template: v8::Local<'_, v8::ObjectTemplate>,
    data: v8::Local<'_, v8::Value>,
    setter: bool,
) {
    assert!(
        checked::<T>(scope).is_some(),
        "a state callback's template is made where the state is one"
    );
    let configuration = v8::IndexedPropertyHandlerConfiguration::new()
        .getter_raw(info_indexed_trampoline::<T, G>)
        .query(indexed_query::<T, G>)
        .enumerator(indexed_enumerate::<T, G>)
        .data(data);
    template.set_indexed_property_handler(if setter {
        configuration.setter(indexed_set_trampoline::<T, S>)
    } else {
        configuration
    });
}

/// Gives `template` (its isolate's) the indexed getter of `G` and the
/// indexed setter of `S`, given the isolate's state, each callback given
/// `data` (`PropertyCallbackArguments::data`).
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn set_state_indexed_properties<T: Any, G: StateIndexedGetter<T>, S: StateIndexedSetter<T>>(
    scope: &mut v8::PinScope<'_, '_>,
    template: v8::Local<'_, v8::ObjectTemplate>,
    data: v8::Local<'_, v8::Value>,
) {
    assert!(
        checked::<T>(scope).is_some(),
        "a state callback's template is made where the state is one"
    );
    template.set_indexed_property_handler(
        v8::IndexedPropertyHandlerConfiguration::new()
            .getter(indexed_trampoline::<T, G>)
            .setter(indexed_set_trampoline::<T, S>)
            .query(indexed_query::<T, G>)
            .enumerator(indexed_enumerate::<T, G>)
            .data(data),
    );
}

fn indexed_set_trampoline<'s, T: Any, S: StateIndexedSetter<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    value: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    _: v8::ReturnValue<'_, v8::Boolean>,
) -> v8::Intercepted {
    S::set(state_scope(scope), index, value, args)
}

/// Gives `template` (its isolate's) the named properties of `N`, given the
/// isolate's state: string names (not symbols) and array indices alike,
/// each callback given `data` (`PropertyCallbackArguments::data`).
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn set_state_named_properties<T: Any, N: StateNamedProperties<T>>(
    scope: &mut v8::PinScope<'_, '_>,
    template: v8::Local<'_, v8::ObjectTemplate>,
    options: NamedOptions,
    data: v8::Local<'_, v8::Value>,
) {
    assert!(
        checked::<T>(scope).is_some(),
        "a state callback's template is made where the state is one"
    );
    let mut flags = v8::PropertyHandlerFlags::ONLY_INTERCEPT_STRINGS;
    let mut index_flags = v8::PropertyHandlerFlags::NONE;
    if !options.masking {
        flags = flags | v8::PropertyHandlerFlags::NON_MASKING;
        index_flags = v8::PropertyHandlerFlags::NON_MASKING;
    }
    let named = v8::NamedPropertyHandlerConfiguration::new();
    let mut named = match options.getter_info {
        Some(getter) => named.getter_raw(getter),
        None => named.getter(named_get::<T, N>),
    }
    .query(named_query::<T, N>)
    .data(data)
    .flags(flags);
    let mut indexed = v8::IndexedPropertyHandlerConfiguration::new()
        .getter(index_get::<T, N>)
        .query(index_query::<T, N>)
        .data(data)
        .flags(index_flags);
    if options.setter {
        named = match options.setter_info {
            Some(setter) => named.setter_raw(setter),
            None => named.setter(named_set::<T, N>),
        }
        .definer(named_define::<T, N>);
        indexed = indexed
            .setter(index_set::<T, N>)
            .definer(index_define::<T, N>);
    }
    if options.deleter {
        named = named.deleter(named_delete::<T, N>);
        indexed = indexed.deleter(index_delete::<T, N>);
    }
    if options.enumerator {
        named = named.enumerator(named_enumerate::<T, N>);
    }
    template.set_named_property_handler(named);
    if options.indices {
        template.set_indexed_property_handler(indexed);
    }
}

/// An object reached from other origins (HTML's cross-origin objects: a
/// `Window`'s, a `Location`'s), given its isolate's state: what
/// `set_state_access_check` and `set_state_refused_access` call. V8 asks
/// `may_access` (always for an object; for a global proxy, only when the
/// realms' security tokens differ); refused, a property goes through the
/// others instead of the object's (Blink's `V8Window` and `V8Location`
/// cross-origin interceptors): string names, symbols and array indices
/// alike.
pub trait StateAccessCheck<T>: 'static {
    /// Whether script of `accessing` may reach `accessed`.
    fn may_access<'s>(
        scope: StateScope<'_, 's, '_, T>,
        accessing: v8::Local<'s, v8::Context>,
        accessed: v8::Local<'s, v8::Object>,
    ) -> bool;

    fn get<'s>(
        scope: StateScope<'_, 's, '_, T>,
        key: PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        rv: v8::ReturnValue<'_, v8::Value>,
    ) -> v8::Intercepted;

    /// The property's attributes (`PropertyAttribute`'s bits).
    fn query<'s>(
        scope: StateScope<'_, 's, '_, T>,
        key: PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        rv: v8::ReturnValue<'_, v8::Integer>,
    ) -> v8::Intercepted;

    fn set<'s>(
        scope: StateScope<'_, 's, '_, T>,
        key: PropertyKey<'s>,
        value: v8::Local<'s, v8::Value>,
        args: v8::PropertyCallbackArguments<'s>,
    ) -> v8::Intercepted;

    /// The property's descriptor, an object (as
    /// `Object.getOwnPropertyDescriptor` gives).
    fn descriptor<'s>(
        scope: StateScope<'_, 's, '_, T>,
        key: PropertyKey<'s>,
        args: v8::PropertyCallbackArguments<'s>,
        rv: v8::ReturnValue<'_, v8::Value>,
    ) -> v8::Intercepted;

    /// The names (`indices` false: strings and symbols) or the array
    /// indices (`indices` true).
    fn enumerate<'s>(
        scope: StateScope<'_, 's, '_, T>,
        indices: bool,
        args: v8::PropertyCallbackArguments<'s>,
        rv: v8::ReturnValue<'_, v8::Array>,
    );

    /// What else refused throws (a delete, a define, a prototype set: V8
    /// calls no interceptor for them) on `target`, in the accessing realm.
    fn refused<'s>(scope: StateScope<'_, 's, '_, T>, target: v8::Local<'s, v8::Object>);
}

extern "C" fn access_check<T: Any, A: StateAccessCheck<T>>(
    accessing: v8::Local<'_, v8::Context>,
    accessed: v8::Local<'_, v8::Object>,
    _: v8::Local<'_, v8::Value>,
) -> bool {
    // SAFETY: V8 calls this on the isolate's thread, while script of
    // `accessing` runs (an access of its), so a callback scope may be
    // opened on it.
    v8::callback_scope!(unsafe scope, accessing);
    let accessed = v8::Local::new(scope, accessed);
    let accessing = v8::Local::new(scope, accessing);
    A::may_access(state_scope(scope), accessing, accessed)
}

extern "C" fn refused_access<T: Any, A: StateAccessCheck<T>>(
    target: v8::Local<'_, v8::Object>,
    _: v8::AccessType,
    _: v8::Local<'_, v8::Value>,
) {
    // SAFETY: as `access_check`'s: V8 calls this on the isolate's thread,
    // as script's access is refused.
    v8::callback_scope!(unsafe scope, target);
    let target = v8::Local::new(scope, target);
    A::refused(state_scope(scope), target);
}

fn check_get<'s, T: Any, A: StateAccessCheck<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Value>,
) -> v8::Intercepted {
    A::get(state_scope(scope), PropertyKey::Name(name), args, rv)
}

fn check_query<'s, T: Any, A: StateAccessCheck<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Integer>,
) -> v8::Intercepted {
    A::query(state_scope(scope), PropertyKey::Name(name), args, rv)
}

fn check_set<'s, T: Any, A: StateAccessCheck<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    value: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    _: v8::ReturnValue<'_, v8::Boolean>,
) -> v8::Intercepted {
    A::set(state_scope(scope), PropertyKey::Name(name), value, args)
}

fn check_descriptor<'s, T: Any, A: StateAccessCheck<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Value>,
) -> v8::Intercepted {
    A::descriptor(state_scope(scope), PropertyKey::Name(name), args, rv)
}

fn check_names<'s, T: Any, A: StateAccessCheck<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Array>,
) {
    A::enumerate(state_scope(scope), false, args, rv);
}

fn check_index_get<'s, T: Any, A: StateAccessCheck<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Value>,
) -> v8::Intercepted {
    A::get(state_scope(scope), PropertyKey::Index(index), args, rv)
}

fn check_index_query<'s, T: Any, A: StateAccessCheck<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Integer>,
) -> v8::Intercepted {
    A::query(state_scope(scope), PropertyKey::Index(index), args, rv)
}

fn check_index_set<'s, T: Any, A: StateAccessCheck<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    value: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    _: v8::ReturnValue<'_, v8::Boolean>,
) -> v8::Intercepted {
    A::set(state_scope(scope), PropertyKey::Index(index), value, args)
}

fn check_index_descriptor<'s, T: Any, A: StateAccessCheck<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Value>,
) -> v8::Intercepted {
    A::descriptor(state_scope(scope), PropertyKey::Index(index), args, rv)
}

fn check_indices<'s, T: Any, A: StateAccessCheck<T>>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<'_, v8::Array>,
) {
    A::enumerate(state_scope(scope), true, args, rv);
}

/// Gives `template` (its isolate's) the access check of `A` and, for what
/// it refuses, `A`'s interceptors, given the isolate's state, each given
/// `data` (`PropertyCallbackArguments::data`).
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn set_state_access_check<T: Any, A: StateAccessCheck<T>>(
    scope: &mut v8::PinScope<'_, '_>,
    template: v8::Local<'_, v8::ObjectTemplate>,
    data: v8::Local<'_, v8::Value>,
) {
    assert!(
        checked::<T>(scope).is_some(),
        "a state callback's template is made where the state is one"
    );
    let named = v8::NamedPropertyHandlerConfiguration::new()
        .getter(check_get::<T, A>)
        .query(check_query::<T, A>)
        .setter(check_set::<T, A>)
        .descriptor(check_descriptor::<T, A>)
        .enumerator(check_names::<T, A>)
        .data(data);
    let indexed = v8::IndexedPropertyHandlerConfiguration::new()
        .getter(check_index_get::<T, A>)
        .query(check_index_query::<T, A>)
        .setter(check_index_set::<T, A>)
        .descriptor(check_index_descriptor::<T, A>)
        .enumerator(check_indices::<T, A>)
        .data(data);
    template.set_access_check_callback_and_handler(
        access_check::<T, A>,
        named,
        indexed,
        Some(data),
    );
}

/// What an access the isolate's access checks refuse and no interceptor
/// takes throws: `A::refused`'s (else V8's `TypeError`).
///
/// # Panics
///
/// If the isolate's state isn't a `T`.
pub fn set_state_refused_access<T: Any, A: StateAccessCheck<T>>(isolate: &mut v8::Isolate) {
    assert!(
        checked::<T>(isolate).is_some(),
        "a state callback's isolate is one whose state is a `T`"
    );
    isolate.set_failed_access_check_callback_function(refused_access::<T, A>);
}
