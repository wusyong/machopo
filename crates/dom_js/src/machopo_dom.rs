//! `machopoDom`: the DOM in a realm with no window, for its tests and for
//! measuring it (`bench.py calls`). `machopoDom.createDocument()` makes an HTML document
//! of the isolate's DOM; its nodes' wrappers are the new bindings'
//! (`templates`), whose interface objects aren't the realm's (`install`
//! puts them there, in a realm of their own): they are `machopoDom`'s
//! (`machopoDom.Event`), as lazily.

use crate::front::wrap_new;
use crate::state::{self, State};
use crate::templates::{self, string};

/// Installs `machopoDom` on `global`: not enumerable, as an interface
/// object. The isolate's DOM is made with the first.
pub fn install_machopo_dom(scope: &mut v8::PinScope<'_, '_>, global: v8::Local<'_, v8::Object>) {
    state::install(scope);
    let next = v8::Object::new(scope);
    let create = v8::Function::builder(create_document)
        .length(0)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .expect("a function");
    let name = string(scope, "createDocument");
    next.set(scope, name.into(), create.into());
    templates::install(scope, next);
    let name = string(scope, "machopoDom");
    global.define_own_property(
        scope,
        name.into(),
        next.into(),
        v8::PropertyAttribute::DONT_ENUM,
    );
}

/// `machopoDom.createDocument()`: a new HTML document, its wrapper made
/// before the operation that makes it (`[NewObject]`).
fn create_document(
    scope: &mut v8::PinScope<'_, '_>,
    _: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let mut scope = state::scope(scope).expect("machopoDom made the isolate's DOM");
    let interface = dom_api::node_interface(dom::NodeKind::Document);
    let wrapper = templates::new_wrapper(&mut scope, interface);
    {
        let (state, mut no_gc) = scope.state_mut();
        state.dom.mutate(|cx| {
            let document = cx.create_document(true);
            wrap_new(&mut no_gc, cx, wrapper, document);
        });
        let made = &state.caches.wrappers_made;
        made.set(made.get().wrapping_add(1));
    }
    state::report(&mut scope);
    rv.set(wrapper.into());
}

/// The wrappers the isolate's DOM made so far, for the tests: 0 with
/// none.
pub fn wrappers_made(isolate: &mut v8::Isolate) -> u64 {
    bindings::isolate_state_mut::<State>(isolate)
        .map_or(0, |state| state.caches.wrappers_made.get())
}

/// The wrappers made ahead of a new node that became its wrapper in the
/// operation that made it (`createElement`'s, `cloneNode`'s), for the
/// tests.
pub fn wrappers_ahead(isolate: &mut v8::Isolate) -> u64 {
    bindings::isolate_state_mut::<State>(isolate)
        .map_or(0, |state| state.caches.wrappers_ahead.get())
}

/// The wrappers made ahead of a new node that weren't its interface's
/// (`createElement`'s foreseen by the name), for the tests.
pub fn wrappers_wasted(isolate: &mut v8::Isolate) -> u64 {
    bindings::isolate_state_mut::<State>(isolate)
        .map_or(0, |state| state.caches.wrappers_wasted.get())
}

/// The names' V8 strings the isolate's DOM made so far (one a name
/// returned), for the tests: 0 with none.
pub fn names_made(isolate: &mut v8::Isolate) -> u64 {
    bindings::isolate_state_mut::<State>(isolate).map_or(0, |state| state.names_made)
}

/// The entries of the isolate's DOM's string cache (one a string of the
/// DOM's whose V8 string was made, the collected ones not yet dropped
/// among them), for the tests: 0 with none.
pub fn cached_strings(isolate: &mut v8::Isolate) -> usize {
    bindings::isolate_state_mut::<State>(isolate).map_or(0, |state| state.caches.strings.len())
}

/// The members' string arguments the isolate's DOM converted by `ToString`
/// so far (none that was a string already), for the tests: 0 with none.
pub fn string_conversions(isolate: &mut v8::Isolate) -> u64 {
    bindings::isolate_state_mut::<State>(isolate).map_or(0, |state| state.string_conversions)
}

/// The exceptions the isolate's listeners and timers threw so far that no
/// `error` listener canceled (logged), for the tests: 0 with no DOM.
pub fn uncaught_exceptions(isolate: &mut v8::Isolate) -> u64 {
    bindings::isolate_state_mut::<State>(isolate).map_or(0, |state| state.uncaught_exceptions)
}

/// The isolate's DOM, for the tests that play its host (an image's
/// fetch handed back): none with no DOM.
pub fn dom_mut(isolate: &mut v8::Isolate) -> Option<&mut dom::Dom> {
    bindings::isolate_state_mut::<State>(isolate).map(|state| &mut state.dom)
}

/// The isolate's DOM's heap's counts, for the tests: none with no DOM.
pub fn heap_stats(isolate: &mut v8::Isolate) -> Option<elise::HeapStats> {
    bindings::isolate_state_mut::<State>(isolate).map(|state| state.dom.stats())
}
