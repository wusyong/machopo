//! A window's named properties object (WebIDL's, for `[Global]` `Window`;
//! HTML's named access on the `Window` object; Blink's `V8Window`'s
//! `WindowProperties`): between `Window.prototype` and
//! `EventTarget.prototype`, one per realm, a named interceptor that isn't
//! asked of a property found otherwise (V8's non-masking interceptor), so
//! a global found on the global or a prototype costs it nothing: what it
//! finds is `dom_api::named_property`'s, a child window, an element
//! or a live collection.

use dom::{ObjectRoot, Window};
use dom_api::{ApiCx, NamedProperty};

use crate::state::{self, Scope};

/// Makes the current realm's named properties object and puts it between
/// `window_prototype` (`Window.prototype`) and its prototype.
pub(crate) fn install<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window_prototype: v8::Local<'s, v8::Object>,
) -> v8::Local<'s, v8::Object> {
    let template = v8::ObjectTemplate::new(scope);
    template.set_named_property_handler(
        v8::NamedPropertyHandlerConfiguration::new()
            .getter(getter)
            .definer(definer)
            .deleter(deleter)
            .flags(
                v8::PropertyHandlerFlags::NON_MASKING
                    | v8::PropertyHandlerFlags::ONLY_INTERCEPT_STRINGS,
            ),
    );
    // Its class string (WebIDL: `@@toStringTag`, not writable, not
    // enumerable, configurable).
    let tag = v8::Symbol::get_to_string_tag(scope);
    let name = crate::templates::string(scope, "WindowProperties");
    template.set_with_attr(
        tag.into(),
        name.into(),
        v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_ENUM,
    );
    let object = template
        .new_instance(scope)
        .expect("a named properties object");
    let event_target_prototype = window_prototype
        .get_prototype(scope)
        .expect("Window.prototype has EventTarget.prototype");
    object.set_prototype(scope, event_target_prototype);
    window_prototype.set_prototype(scope, object.into());
    object
}

/// A realm's named properties object, kept by its context (weakly: a slot
/// holds no realm alive), for when its frame navigates on (`detached`).
struct NamedProperties(v8::Weak<v8::Object>);

/// The private key of a realm's document on its named properties object,
/// once the realm's global proxy has gone on to the next one.
fn document_key<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Private> {
    let name = crate::templates::string(scope, "machopo:realm-document");
    v8::Private::for_api(scope, Some(name))
}

/// `context`'s frame navigates on: its global proxy goes to the next
/// realm, whose window it then is. What the realm's script finds by name
/// stays its own window's (HTML's: the `Window`'s associated `Document`,
/// as Blink's `WindowProperties` finds its `LocalDOMWindow` by its
/// holder's creation context): its document kept on its named properties
/// object, which the lookup reads first.
pub(crate) fn detached(scope: &mut Scope<'_, '_, '_>, context: v8::Local<'_, v8::Context>) {
    let Some(named) = context.get_slot::<NamedProperties>() else {
        return;
    };
    let Some(object) = named.0.to_local(scope.scope()) else {
        return;
    };
    let global = context.global(scope.scope());
    let document = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            let window = no_gc.unwrap::<Window>(cx.gc, global)?;
            let document = cx.window_document(window);
            let api = ApiCx::with_layout_cell(cx, &state.layout);
            let object = dom::ObjectRef::Node(document);
            Some((cx.root_object(object), dom_api::interface_of(&api, object)))
        })
    };
    let Some((document, interface)) = document else {
        return;
    };
    let wrapper = crate::front::wrapper_of(scope, &document, interface);
    let scope = scope.scope();
    let key = document_key(scope);
    object.set_private(scope, key, wrapper.into());
}

/// What the lookup found, rooted for after it.
enum Found {
    None,
    Object(ObjectRoot, usize),
    /// Several elements: their collection is made (`make`).
    Several(elise::Persistent<Window>),
}

/// The window's named object called `name`, in the realm the object is in
/// (its holder's: the window whose global it is under); with `make`, its
/// collection made if there are several.
fn named<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: &v8::PropertyCallbackArguments<'s>,
    make: bool,
) -> Option<Option<v8::Local<'s, v8::Object>>> {
    let name = name.to_rust_string_lossy(scope);
    let context = args.holder().get_creation_context(scope)?;
    let global = context.global(scope);
    // A realm its frame navigated on from: its own document's window.
    let key = document_key(scope);
    let document = args
        .holder()
        .get_private(scope, key)
        .and_then(|document| v8::Local::<v8::Object>::try_from(document).ok());
    let mut scope = state::scope(scope)?;
    let found = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            let window = match document {
                Some(document) => no_gc
                    .unwrap::<dom::Node>(cx.gc, document)
                    .and_then(|document| cx.window_of(document)),
                None => no_gc.unwrap::<Window>(cx.gc, global),
            };
            let Some(window) = window else {
                return Found::None;
            };
            match dom_api::named_property(cx, window, &name) {
                NamedProperty::None => Found::None,
                NamedProperty::Object(object) => {
                    let api = ApiCx::with_layout_cell(cx, &state.layout);
                    Found::Object(cx.root_object(object), dom_api::interface_of(&api, object))
                }
                NamedProperty::Several => Found::Several(elise::Persistent::new(cx.gc, window)),
            }
        })
    };
    let (object, interface) = match found {
        Found::None => return Some(None),
        Found::Object(object, interface) => (object, interface),
        Found::Several(_) if !make => {
            // Only whether there is one is asked: the getter makes it.
            return Some(Some(args.holder()));
        }
        Found::Several(window) => {
            let made = {
                let (state, _) = scope.state_mut();
                state.dom.mutate(|cx| {
                    let window = window.get(cx.gc);
                    let collection = dom_api::named_property_collection(cx, window, &name);
                    let api = ApiCx::with_layout_cell(cx, &state.layout);
                    (
                        cx.root_object(collection),
                        dom_api::interface_of(&api, collection),
                    )
                })
            };
            state::report(&mut scope);
            made
        }
    };
    Some(Some(crate::front::wrapper_of(
        &mut scope, &object, interface,
    )))
}

fn getter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
    match named(scope, name, &args, true).flatten() {
        Some(value) => {
            rv.set(value.into());
            v8::Intercepted::kYes
        }
        None => v8::Intercepted::kNo,
    }
}

/// Setting or defining any property on the object itself fails (WebIDL's
/// [[DefineOwnProperty]] of a named properties object, which [[Set]] comes
/// to with the object as the receiver: V8 sends a set without a setter
/// here).
fn definer<'s>(
    _: &mut v8::PinScope<'s, '_>,
    _: v8::Local<'s, v8::Name>,
    _: &v8::PropertyDescriptor,
    _: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    rv.set_bool(false);
    v8::Intercepted::kYes
}

/// Deleting a named property fails (WebIDL's [[Delete]]); another name is
/// an ordinary property's.
fn deleter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    match named(scope, name, &args, false).flatten() {
        Some(_) => {
            rv.set_bool(false);
            v8::Intercepted::kYes
        }
        None => v8::Intercepted::kNo,
    }
}

/// `scope`'s realm's `Window.prototype`, its named properties object put
/// under it (`install`).
pub(crate) fn install_in_realm(scope: &mut Scope<'_, '_, '_>, global: v8::Local<'_, v8::Object>) {
    let scope = scope.scope();
    let key = crate::templates::string(scope, "Window");
    let prototype = global
        .get(scope, key.into())
        .and_then(|window| v8::Local::<v8::Object>::try_from(window).ok())
        .and_then(|window| {
            let key = crate::templates::string(scope, "prototype");
            window.get(scope, key.into())
        })
        .and_then(|prototype| v8::Local::<v8::Object>::try_from(prototype).ok())
        .expect("Window has a prototype");
    let object = install(scope, prototype);
    let weak = v8::Weak::new(scope, object);
    // An `Rc`: what rusty_v8's context slots hold (read once, as the frame
    // navigates on).
    scope
        .get_current_context()
        .set_slot(std::rc::Rc::new(NamedProperties(weak)));
}
