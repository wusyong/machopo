//! The embedder's state in an isolate data slot: set once, borrowed as its
//! type only and only through the isolate, dropped by the isolate's owner;
//! a callback on a template made where the state is its type borrows it.

#![allow(unsafe_code, reason = "dropping the state is the owner's promise")]

use std::cell::Cell;
use std::rc::Rc;

use bindings::{
    StateCallback, StateIndexedGetter, StateScope, V8PerIsolateData, drop_isolate_state,
    isolate_state_mut, set_isolate_state, set_state_indexed_getter, state_function_template,
};

struct State {
    value: u32,
    drops: Rc<Cell<u32>>,
}

impl Drop for State {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

#[test]
fn the_state_is_borrowed_as_its_type_and_dropped_by_the_owner() {
    let mut data = V8PerIsolateData::new();
    let isolate = data.isolate();
    assert!(isolate_state_mut::<State>(isolate).is_none());
    let drops = Rc::new(Cell::new(0));
    set_isolate_state(
        isolate,
        State {
            value: 7,
            drops: drops.clone(),
        },
    );
    let state = isolate_state_mut::<State>(isolate).unwrap();
    assert_eq!(state.value, 7);
    state.value = 8;
    // Another borrow is the same state.
    assert_eq!(isolate_state_mut::<State>(isolate).unwrap().value, 8);
    // Of another type: none.
    assert!(isolate_state_mut::<u32>(isolate).is_none());
    assert_eq!(drops.get(), 0);
    // SAFETY: no callback of the isolate runs after.
    unsafe { drop_isolate_state(isolate) };
    assert_eq!(drops.get(), 1);
    assert!(isolate_state_mut::<State>(isolate).is_none());
    // Dropping none does nothing; it can be set again then.
    // SAFETY: as above.
    unsafe { drop_isolate_state(isolate) };
    set_isolate_state(isolate, 5u32);
    assert_eq!(isolate_state_mut::<u32>(isolate).copied(), Some(5));
    // SAFETY: as above.
    unsafe { drop_isolate_state(isolate) };
}

#[test]
#[should_panic(expected = "an isolate's state is set once")]
fn the_state_is_set_once() {
    let mut data = V8PerIsolateData::new();
    let isolate = data.isolate();
    set_isolate_state(isolate, 1u32);
    set_isolate_state(isolate, 2u32);
}

/// The isolate's owner drops its state as it goes, before the isolate.
#[test]
fn the_isolates_owner_drops_its_state() {
    let drops = Rc::new(Cell::new(0));
    let mut data = V8PerIsolateData::new();
    set_isolate_state(
        data.isolate(),
        State {
            value: 0,
            drops: drops.clone(),
        },
    );
    drop(data);
    assert_eq!(drops.get(), 1);
}

/// Counts its calls in the state, and returns the count.
struct Count;

impl StateCallback<State> for Count {
    fn call<'s>(
        mut scope: StateScope<'_, 's, '_, State>,
        _: v8::FunctionCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'s>,
    ) {
        let (state, _) = scope.state_mut();
        state.value += 1;
        let value = state.value;
        assert_eq!(scope.state().0.value, value, "a shared borrow sees it");
        rv.set_uint32(value);
    }
}

/// Returns the index plus the state's value; its properties are at the
/// indices below the state's value, read-only.
struct Plus;

impl StateIndexedGetter<State> for Plus {
    fn get<'s>(
        mut scope: StateScope<'_, 's, '_, State>,
        index: u32,
        _: v8::PropertyCallbackArguments<'s>,
        _: bindings::Receiver<'s>,
        mut rv: v8::ReturnValue<'_, v8::Value>,
    ) -> v8::Intercepted {
        rv.set_uint32(index + scope.state().0.value);
        v8::Intercepted::kYes
    }

    fn query<'s>(
        mut scope: StateScope<'_, 's, '_, State>,
        index: u32,
        _: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Integer>,
    ) -> v8::Intercepted {
        if index >= scope.state().0.value {
            return v8::Intercepted::kNo;
        }
        rv.set_int32(v8::PropertyAttribute::READ_ONLY.as_u32().cast_signed());
        v8::Intercepted::kYes
    }

    fn enumerate<'s>(
        mut scope: StateScope<'_, 's, '_, State>,
        _: v8::PropertyCallbackArguments<'s>,
        mut rv: v8::ReturnValue<'_, v8::Array>,
    ) {
        let count = scope.state().0.value;
        let scope = scope.scope();
        let indices: Vec<v8::Local<'_, v8::Value>> = (0..count)
            .map(|index| v8::Integer::new_from_unsigned(scope, index).into())
            .collect();
        rv.set(v8::Array::new_with_elements(scope, &indices));
    }
}

/// A callback on a template made where the state is its type borrows it
/// (with no check), shared or not; so does an indexed getter; out of
/// such a callback, a scope's borrow is checked. The getter's queries are
/// its own: `Object.keys` its indices, their attributes its.
#[test]
fn a_callback_borrows_its_isolates_state() {
    let mut data = V8PerIsolateData::new();
    let drops = Rc::new(Cell::new(0));
    set_isolate_state(data.isolate(), State { value: 0, drops });
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let count = state_function_template::<State, Count>(scope, |builder| builder);
    let count = count.get_function(scope).unwrap();
    let holder = v8::ObjectTemplate::new(scope);
    let data = v8::undefined(scope);
    set_state_indexed_getter::<State, Plus>(scope, holder, data.into());
    let holder = holder.new_instance(scope).unwrap();
    let global = context.global(scope);
    let name = v8::String::new(scope, "count").unwrap();
    global.set(scope, name.into(), count.into());
    let name = v8::String::new(scope, "holder").unwrap();
    global.set(scope, name.into(), holder.into());
    let source = v8::String::new(
        scope,
        "count(); count(); [count(), holder[10], Object.keys(holder).join('|'), \
         Object.getOwnPropertyDescriptor(holder, 1).writable, 5 in holder].join()",
    )
    .unwrap();
    let script = v8::Script::compile(scope, source, None).unwrap();
    let result = script.run(scope).unwrap().to_rust_string_lossy(scope);
    assert_eq!(result, "3,13,0|1|2,false,false");
    assert!(StateScope::<u32>::new(scope).is_none(), "another type");
    let mut checked = StateScope::<State>::new(scope).expect("the state");
    assert_eq!(checked.state().0.value, 3);
}

/// A state callback's template is made only where the state is its type.
#[test]
#[should_panic(expected = "a state callback's template is made where the state is one")]
fn a_state_callbacks_template_needs_the_state() {
    let mut data = V8PerIsolateData::new();
    set_isolate_state(data.isolate(), 1u32);
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    state_function_template::<State, Count>(scope, |builder| builder);
}
