//! Custom elements (HTML's): a window's `customElements` and its `define`,
//! `get`, `getName` and `whenDefined` (Blink's `CustomElementRegistry`).
//! `dom` keeps the registry and its definitions; what `define` reads
//! off a constructor, and holding and comparing it, are the engine's
//! (`ScriptHost::read_custom_element_constructor`); the rules are here:
//! `HTMLElement`'s constructor (`[HTMLConstructor]`), what `createElement`
//! checks of what a constructor made, and the upgrade.
//! Autonomous custom elements only: a definition that `extends` a built-in
//! element is a `NotSupportedError`.

use dom::{
    CallbackArgs, ConstructionTop, CustomElementDefinition, CustomElementRegistry,
    CustomElementState, DefinitionInit, DomString, NameIds, Node, NodeKind, NodeRef, ReactionRef,
    Window, names,
};
use elise::{Gc, Persistent};

use crate::exception::ExceptionName;
use crate::interfaces::ElementDefinitionOptions;
use crate::typed::{Callback, Created, HtmlConstructed, PromiseOf};
use crate::{ApiCx, ApiError, DomStr, NullableStr, OpFront, ScriptHost};

/// `window.customElements`: the same object each time.
pub(crate) fn registry<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<Gc<'gc, CustomElementRegistry>, ApiError> {
    Ok(cx.custom_elements(this))
}

fn not_supported(message: &str) -> ApiError {
    ApiError::Dom(crate::exception::DomException {
        name: ExceptionName::NotSupported,
        message: message.to_owned(),
    })
}

/// `define(name, constructor, options)`: HTML's "define", for autonomous
/// custom elements. The checks first (a constructor, a valid name, neither
/// defined yet, no `extends`, no `define` running), then the constructor
/// is read with the registry's "element definition is running" set; the
/// definition goes in, and the promise `whenDefined(name)` gave resolves
/// with the constructor.
pub(crate) fn define<H: ScriptHost, C>(
    host: &mut H,
    this: Persistent<CustomElementRegistry>,
    name: String,
    constructor: Callback<C>,
    options: ElementDefinitionOptions,
) -> Result<(), ApiError> {
    let _span = trace_event::trace_event!("CustomElementRegistry::define");
    let _ = constructor;
    if !host.constructor_is_constructor() {
        return Err(ApiError::type_error(
            "define: the constructor is not a constructor",
        ));
    }
    if !dom::is_valid_custom_element_name(&name) {
        return Err(ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::Syntax,
            message: format!("define: \"{name}\" is not a valid custom element name"),
        }));
    }
    let id = names::intern(&name);
    if host.read(|cx| cx.custom_element_definition(this.get(cx.gc), id).is_some()) {
        return Err(not_supported(&format!(
            "define: \"{name}\" is defined already"
        )));
    }
    if host.constructor_is_defined(&this) {
        return Err(not_supported("define: the constructor is defined already"));
    }
    if options.extends.is_some() {
        return Err(not_supported(
            "define: customized built-in elements (`extends`) aren't supported",
        ));
    }
    if host.read(|cx| cx.element_definition_is_running(this.get(cx.gc))) {
        return Err(not_supported("define: another definition is being made"));
    }
    host.mutate(|cx| cx.set_element_definition_is_running(this.get(cx.gc), true));
    let read = host.read_custom_element_constructor();
    host.mutate(|cx| cx.set_element_definition_is_running(this.get(cx.gc), false));
    let info = read?;
    let (definition, pending) = host.mutate(|cx| {
        let registry = this.get(cx.gc);
        let definition = cx.add_custom_element_definition(
            registry,
            DefinitionInit {
                name: id,
                local_name: id,
                observed: info
                    .observed_attributes
                    .iter()
                    .map(|name| names::intern(name))
                    .collect(),
                disable_internals: info
                    .disabled_features
                    .iter()
                    .any(|feature| feature == "internals"),
                disable_shadow: info
                    .disabled_features
                    .iter()
                    .any(|feature| feature == "shadow"),
            },
        );
        let pending = cx.take_when_defined(registry, id);
        (Persistent::new(cx.gc, definition), pending)
    });
    host.store_custom_element_definition(&this, &definition);
    if !pending.is_empty() {
        host.resolve_when_defined(&pending, &definition);
    }
    // The candidates' upgrade reactions, invoked as `define` returns
    // (`[CEReactions]`).
    host.mutate(|cx| {
        let definition = definition.get(cx.gc);
        for element in cx.take_upgrade_candidates(this.get(cx.gc), id) {
            cx.enqueue_upgrade_reaction(element, definition);
        }
    });
    Ok(())
}

/// HTML's "invoke custom element reactions" of an element queue: each
/// element's reactions, in order, those it gets as they run too, before
/// the next element's.
pub(crate) fn invoke_reactions<H: ScriptHost>(
    host: &mut H,
    queue: &Persistent<elise::HeapVector<Node>>,
) {
    while let Some(element) = host.mutate(|cx| {
        cx.take_from_element_queue(queue)
            .map(|element| Persistent::new(cx.gc, element))
    }) {
        invoke_element_reactions(host, &element);
    }
}

/// The element queue of the `[CEReactions]`-like scope last left, if it
/// left one (`Cx::leave_ce_reactions` gave true): its reactions invoked
/// (the parser's construction's, out of any member's call).
pub fn invoke_left_ce_reactions<H: ScriptHost>(host: &mut H) {
    if let Some(queue) = host.mutate(|cx| cx.take_left_element_queue()) {
        invoke_reactions(host, &queue);
    }
}

/// The backup element queue's reactions (HTML's microtask processing it),
/// the elements enqueued as they run too.
pub fn process_backup_element_queue<H: ScriptHost>(host: &mut H) {
    while let Some(element) = host.mutate(|cx| {
        cx.take_from_backup_element_queue()
            .map(|element| Persistent::new(cx.gc, element))
    }) {
        invoke_element_reactions(host, &element);
    }
}

/// A reaction taken off an element's queue, rooted for its invocation.
enum Rooted {
    Upgrade(Persistent<CustomElementDefinition>),
    Callback(Persistent<CustomElementDefinition>, LifecycleCallback),
}

/// A lifecycle callback's call (`ScriptHost::call_lifecycle_callback`):
/// which, and its arguments.
pub enum LifecycleCallback {
    Connected,
    Disconnected,
    /// The old document and the new.
    Adopted(Persistent<Node>, Persistent<Node>),
    /// The attribute's local name, old value, new value and namespace.
    AttributeChanged {
        name: DomString,
        old: Option<DomString>,
        new: Option<DomString>,
        namespace: Option<DomString>,
    },
}

fn invoke_element_reactions<H: ScriptHost>(host: &mut H, element: &Persistent<Node>) {
    // Chrome's trace's span of them: Blink's queue is the element's.
    let _span = trace_event::trace_event!("CustomElementReactionQueue::invokeReactions");
    loop {
        let reaction = host.mutate(|cx| {
            cx.take_element_reaction(element.get(cx.gc))
                .map(|reaction| match reaction {
                    ReactionRef::Upgrade(definition) => {
                        Rooted::Upgrade(Persistent::new(cx.gc, definition))
                    }
                    ReactionRef::Callback(definition, args) => Rooted::Callback(
                        Persistent::new(cx.gc, definition),
                        match args {
                            CallbackArgs::Connected => LifecycleCallback::Connected,
                            CallbackArgs::Disconnected => LifecycleCallback::Disconnected,
                            CallbackArgs::Adopted(old, new) => LifecycleCallback::Adopted(
                                Persistent::new(cx.gc, old),
                                Persistent::new(cx.gc, new),
                            ),
                            CallbackArgs::AttributeChanged {
                                local,
                                old,
                                new,
                                namespace,
                            } => LifecycleCallback::AttributeChanged {
                                name: names::string(local),
                                old,
                                new,
                                namespace: namespace.map(names::string),
                            },
                        },
                    ),
                })
        });
        match reaction {
            None => return,
            Some(Rooted::Upgrade(definition)) => upgrade(host, &definition, element),
            Some(Rooted::Callback(definition, callback)) => {
                host.call_lifecycle_callback(element, &definition, callback)
            }
        }
    }
}

/// HTML's "upgrade" of `element` with `definition`: its definition set
/// and its state failed until its constructor, `element` on the
/// definition's construction stack, gives it (`ScriptHost::upgrade_element`,
/// which reports what it threw); then custom, or its definition taken away.
pub(crate) fn upgrade<H: ScriptHost>(
    host: &mut H,
    definition: &Persistent<CustomElementDefinition>,
    element: &Persistent<Node>,
) {
    let upgrading = host.mutate(|cx| {
        let (element, definition) = (element.get(cx.gc), definition.get(cx.gc));
        if !matches!(
            cx.custom_element_state(element),
            CustomElementState::Undefined | CustomElementState::Uncustomized
        ) {
            return false;
        }
        cx.set_element_custom_definition(element, definition);
        cx.set_custom_element_state(element, CustomElementState::Failed);
        // Its attributes' and its connection's callbacks, after it.
        for index in 0..cx.attribute_count(element) {
            let attribute = cx
                .attribute_at(element, index)
                .expect("an attribute below the count");
            cx.enqueue_callback_reaction(
                element,
                CallbackArgs::AttributeChanged {
                    local: attribute.name.local,
                    old: None,
                    new: Some(attribute.value),
                    namespace: Some(attribute.name.namespace).filter(|space| *space != names::NONE),
                },
            );
        }
        if cx.is_connected(element) {
            cx.enqueue_callback_reaction(element, CallbackArgs::Connected);
        }
        cx.push_construction(definition, element);
        cx.push_active_constructor(definition);
        true
    });
    if !upgrading {
        return;
    }
    let constructed = host.upgrade_element(definition, element);
    host.mutate(|cx| {
        let (element, definition) = (element.get(cx.gc), definition.get(cx.gc));
        cx.pop_active_constructor();
        cx.pop_construction(definition);
        if constructed {
            cx.set_custom_element_state(element, CustomElementState::Custom);
        } else {
            cx.clear_element_custom_definition(element);
            cx.clear_element_reactions(element);
        }
    });
}

/// `upgrade(root)`: HTML's "try to upgrade" of each of `root`'s
/// shadow-including inclusive descendants with a definition, run as it
/// returns.
pub(crate) fn upgrade_tree<H: ScriptHost>(
    host: &mut H,
    _: Persistent<CustomElementRegistry>,
    root: Persistent<Node>,
) -> Result<(), ApiError> {
    host.mutate(|cx| {
        for (element, definition) in cx.upgradable_under(root.get(cx.gc)) {
            cx.enqueue_upgrade_reaction(element, definition);
        }
    });
    Ok(())
}

fn illegal_constructor() -> ApiError {
    ApiError::type_error("Illegal constructor")
}

/// `HTMLElement`'s constructor (HTML's `[HTMLConstructor]`), for a custom
/// element's `super()`: the definition whose constructor `new.target` is,
/// the running one's (an upgrade's, a synchronous creation's, of any
/// realm's registry) or else in the current realm's window's registry
/// (none, or `HTMLElement` itself: a `TypeError`). With its construction stack empty (`new X()`,
/// `createElement`), a new element of it, custom; else the element being
/// upgraded, marked as constructed (a second `super()`: a `TypeError`).
pub(crate) fn html_constructor<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
) -> Result<HtmlConstructed<'gc>, ApiError> {
    let window = op.current_window(cx).ok_or_else(illegal_constructor)?;
    let number = op.new_target_number().ok_or_else(illegal_constructor)?;
    let running = cx.active_constructor(|definition| {
        op.is_new_target(cx, cx.definition_edges(definition).constructor)
    });
    let definition = match running {
        Some(definition) => definition,
        None => {
            let registry = cx
                .custom_elements_if_made(window)
                .ok_or_else(illegal_constructor)?;
            cx.custom_element_definitions_numbered(registry, number)
                .into_iter()
                .find(|definition| {
                    op.is_new_target(cx, cx.definition_edges(*definition).constructor)
                })
                .ok_or_else(illegal_constructor)?
        }
    };
    match cx.construction_top(definition) {
        None => {
            let document = cx.window_document(window);
            let element = cx.create_element_named(
                document,
                NameIds {
                    namespace: names::HTML_NAMESPACE,
                    prefix: names::NONE,
                    local: cx.definition_local_name(definition),
                },
            );
            cx.set_custom_element_state(element, CustomElementState::Custom);
            cx.set_element_custom_definition(element, definition);
            Ok(HtmlConstructed::New(element))
        }
        Some(ConstructionTop::AlreadyConstructed) => Err(ApiError::type_error(
            "Failed to construct 'HTMLElement': This instance is already constructed",
        )),
        Some(ConstructionTop::Element(element)) => {
            cx.mark_construction_top(definition);
            Ok(HtmlConstructed::Upgrading(element))
        }
    }
}

/// What HTML's upgrade checks before `definition`'s constructor runs for
/// `element`: a `NotSupportedError` if the definition disables shadow
/// roots and `element` has one already (thrown in the constructor's
/// place: the upgrade fails, reported).
pub fn upgradable<'gc>(
    cx: &ApiCx<'_, 'gc>,
    definition: Gc<'gc, CustomElementDefinition>,
    element: NodeRef<'gc>,
) -> Result<(), ApiError> {
    let (_, disable_shadow) = cx.definition_disabled(definition);
    if disable_shadow && cx.shadow_root(element).is_some() {
        return Err(not_supported(
            "Failed to upgrade: the custom element disables shadow and already has a shadow root",
        ));
    }
    Ok(())
}

/// What `createElement` checks of what `definition`'s constructor made in
/// `document` (HTML's "create an element" with the synchronous custom
/// elements flag): an HTML element (else a `TypeError`) with no
/// attributes, no children and no parent, of `document`, with its local
/// name (else a `NotSupportedError`).
pub fn created<'gc>(
    cx: &ApiCx<'_, 'gc>,
    made: Option<NodeRef<'gc>>,
    definition: Gc<'gc, CustomElementDefinition>,
    document: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    let element = made
        .filter(|node| {
            cx.kind(*node) == NodeKind::Element
                && cx
                    .element_name(*node)
                    .is_some_and(|name| name.namespace == names::HTML_NAMESPACE)
        })
        .ok_or_else(|| {
            ApiError::type_error(
                "Failed to execute 'createElement': The result must implement HTMLElement",
            )
        })?;
    let check = |ok: bool, what: &str| {
        if ok {
            Ok(())
        } else {
            Err(not_supported(&format!(
                "Failed to execute 'createElement': The result must not {what}"
            )))
        }
    };
    check(cx.attribute_count(element) == 0, "have attributes")?;
    check(cx.first_child(element).is_none(), "have children")?;
    check(cx.parent(element).is_none(), "have a parent")?;
    check(
        elise::Gc::ptr_eq(cx.document_of(element), document),
        "be of another document",
    )?;
    check(
        cx.element_name(element)
            .is_some_and(|name| name.local == cx.definition_local_name(definition)),
        "have another local name",
    )?;
    Ok(element)
}

/// The element `createElement` gives when constructing threw or what it
/// made failed `created`'s checks: an `HTMLUnknownElement` of
/// `definition`'s name, failed.
pub fn failed_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    definition: Gc<'gc, CustomElementDefinition>,
    document: NodeRef<'gc>,
) -> NodeRef<'gc> {
    let element = cx.create_element_named(
        document,
        NameIds {
            namespace: names::HTML_NAMESPACE,
            prefix: names::NONE,
            local: cx.definition_local_name(definition),
        },
    );
    cx.set_custom_element_state(element, CustomElementState::Failed);
    element
}

/// What `createElement` gives for `element`, undefined, just made in
/// `document`: the custom element to construct after the operation if
/// its name has a definition in the document's window's registry (an HTML
/// element's name with no prefix: `custom_element_definition_for`), else
/// `element`.
#[cold]
#[inline(never)]
pub(crate) fn to_construct<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    element: NodeRef<'gc>,
) -> Created<'gc> {
    match cx.custom_element_definition_for(element) {
        Some(definition) => Created::Construct {
            definition,
            document: cx.document_of(document),
        },
        None => Created::Element(element),
    }
}

/// `get(name)`: the constructor of the definition named `name`, or
/// undefined.
pub(crate) fn get<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CustomElementRegistry>,
    name: &str,
) -> Result<Option<&'gc elise::JsEdge>, ApiError> {
    let Some(id) = names::find(name) else {
        return Ok(None);
    };
    Ok(cx
        .custom_element_definition(this, id)
        .map(|definition| cx.definition_edges(definition).constructor))
}

/// `getName(constructor)`: the name of the definition whose constructor
/// it is, or null.
pub(crate) fn get_name<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CustomElementRegistry>,
    constructor: Callback<O::Callback>,
) -> Result<NullableStr<'gc>, ApiError> {
    let number = op.callback_number(constructor);
    let found = cx
        .custom_element_definitions_numbered(this, number)
        .into_iter()
        .find(|definition| {
            op.same_callback(
                cx,
                cx.definition_edges(*definition).constructor,
                constructor,
            )
        });
    Ok(NullableStr(found.map(|definition| {
        DomStr::Name(cx.definition_name(definition))
    })))
}

/// `whenDefined(name)`: a promise resolved with the constructor of the
/// definition named `name` once there is one (the same promise until
/// then); a `SyntaxError` (rejected) for a name that isn't valid.
pub(crate) fn when_defined<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CustomElementRegistry>,
    name: &str,
) -> Result<PromiseOf<'gc>, ApiError> {
    let Some(id) = cx.custom_element_name(name) else {
        return Err(ApiError::Dom(crate::exception::DomException {
            name: ExceptionName::Syntax,
            message: format!("whenDefined: \"{name}\" is not a valid custom element name"),
        }));
    };
    if let Some(definition) = cx.custom_element_definition(this, id) {
        return Ok(PromiseOf::Defined(definition));
    }
    Ok(match cx.when_defined(this, id) {
        Some(edge) => PromiseOf::Existing(edge),
        None => PromiseOf::Pending(cx.add_when_defined(this, id)),
    })
}
