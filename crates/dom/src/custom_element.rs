//! Custom elements (HTML's; Blink's `core/html/custom/`): a window's
//! `CustomElementRegistry` (`customElements`, made at the first ask, which
//! the window keeps) and its definitions (Blink's
//! `CustomElementDefinition`), each with its constructor and lifecycle
//! callbacks in JS edges (the engine's, which only it reads), its name and
//! observed attributes; and the promises `whenDefined` gave before their
//! name was defined, by a number (the engine keeps their resolvers by it:
//! a JS edge holds an object, and a resolver read back from one is no
//! longer one). A definition is found by its name and by its constructor
//! (Blink's `name_map_` and `constructor_map_`: the engine's number for the
//! constructor, V8's identity hash, then its handle compared), a page with
//! hundreds of them finding one at once.
//!
//! An element's custom element state (HTML's: undefined, failed,
//! uncustomized, precustomized, custom) is in 3 bits of its tree scope
//! byte (Blink's node flags), its definition in its data. An undefined
//! element connected while its name isn't defined is a candidate of its
//! document's registry for that name (Blink's `upgrade_candidates_`): held
//! weakly, a small object's `WeakMember` in a list (the heap has weak edges,
//! no weak containers), so that an element taken out and dropped isn't
//! kept; `define` takes its name's in shadow-including tree order. Each
//! definition's construction stack (HTML's), the elements its upgrades are
//! constructing, the last of which `HTMLElement`'s constructor gives.
//! What `define` reads off a constructor, and the rules, are
//! `dom_api`'s.

use std::cell::Cell;

use elise::{External, Gc, GcObject, HeapVector, JsEdge, Member, WeakMember};
use rustc_hash::FxHashMap;

use crate::{Cx, ElementData, NameId, Node, NodeKind, NodeRef, Window, names, sites};

/// The names a custom element may not have, though they look like one.
const RESERVED_CUSTOM_ELEMENT_NAMES: &[&str] = &[
    "annotation-xml",
    "color-profile",
    "font-face",
    "font-face-src",
    "font-face-uri",
    "font-face-format",
    "font-face-name",
    "missing-glyph",
];

/// Whether `name` has a hyphen, the first of a custom element name's
/// checks on the paths making every element: a loop over its few bytes,
/// inlined (`str::contains` calls `memchr`, which costs a short name more
/// than looking).
#[inline]
pub fn has_hyphen(name: &str) -> bool {
    name.bytes().any(|byte| byte == b'-')
}

/// HTML's "valid custom element name": a lowercase ASCII letter first, a
/// hyphen, no uppercase ASCII, none of what a valid element local name
/// can't have after an ASCII letter (DOM's: NUL, ASCII whitespace but
/// vertical tab, `/`, `>`), not reserved.
pub fn is_valid_custom_element_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.contains('-')
        && !name.contains(|c: char| {
            c.is_ascii_uppercase()
                || matches!(c, '\0' | '\t' | '\n' | '\x0c' | '\r' | ' ' | '/' | '>')
        })
        && !RESERVED_CUSTOM_ELEMENT_NAMES.contains(&name)
}

/// A registry's definitions by their constructors' numbers (several
/// names may share one).
type ByConstructor = FxHashMap<u32, Vec<u32>>;

/// A window's custom element registry: its definitions in the order they
/// came, found by name and by the engine's number for their constructors
/// (their places in `definitions`; a number may be several constructors'),
/// the `whenDefined` promises still pending, whether a `define` is reading
/// a constructor (HTML's "element definition is running"), and the
/// upgrade candidates by name (their lists' places in `candidates`). The
/// maps in a `Cell`, taken and put back (a move, no borrow).
#[derive(GcObject)]
pub struct CustomElementRegistry {
    window: Member<Window>,
    definitions: Member<HeapVector<CustomElementDefinition>>,
    by_name: External<Cell<FxHashMap<NameId, u32>>>,
    by_constructor: External<Cell<ByConstructor>>,
    when_defined: Member<HeapVector<WhenDefined>>,
    running: Cell<bool>,
    candidates: Member<HeapVector<CandidateList>>,
    candidates_by_name: External<Cell<FxHashMap<NameId, u32>>>,
    wrapper: JsEdge,
}

/// A name's upgrade candidates, and how many there may be before the dead
/// ones are let go of.
#[derive(GcObject)]
pub struct CandidateList {
    list: Member<HeapVector<Candidate>>,
    prune_at: Cell<u32>,
}

/// One upgrade candidate, held weakly.
#[derive(GcObject)]
pub struct Candidate {
    element: WeakMember<Node>,
}

/// An element's custom element state (HTML's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomElementState {
    /// Not a custom element: its name isn't a valid custom element name
    /// (most).
    Uncustomized,
    /// A custom element's name, not defined (or not upgraded) yet.
    Undefined,
    /// Its constructor or upgrade threw.
    Failed,
    /// Being made by its constructor (`createElement`'s).
    Precustomized,
    /// Its definition's, constructed.
    Custom,
}

/// Where an element's state is in its tree scope byte (`Node::scope`), 3
/// bits after `shadow`'s.
const STATE_SHIFT: u8 = 3;
const STATE_MASK: u8 = 0b111 << STATE_SHIFT;
const UNDEFINED: u8 = 1 << STATE_SHIFT;

impl CustomElementState {
    fn bits(self) -> u8 {
        let raw: u8 = match self {
            Self::Uncustomized => 0,
            Self::Undefined => 1,
            Self::Failed => 2,
            Self::Precustomized => 3,
            Self::Custom => 4,
        };
        raw << STATE_SHIFT
    }

    fn from_bits(bits: u8) -> Self {
        match (bits & STATE_MASK) >> STATE_SHIFT {
            0 => Self::Uncustomized,
            1 => Self::Undefined,
            2 => Self::Failed,
            3 => Self::Precustomized,
            _ => Self::Custom,
        }
    }
}

/// The last of a definition's construction stack.
#[derive(Clone, Copy)]
pub enum ConstructionTop<'gc> {
    Element(NodeRef<'gc>),
    /// HTML's "already constructed marker": its constructor called
    /// `super()` once already.
    AlreadyConstructed,
}

/// One definition: its name and local name (the same: autonomous ones
/// only), its constructor and lifecycle callbacks (`connectedCallback`,
/// `disconnectedCallback`, `adoptedCallback`, `attributeChangedCallback`;
/// an edge with none, a callback the prototype doesn't have), its observed
/// attributes, and what its `disabledFeatures` turn off.
#[derive(GcObject)]
pub struct CustomElementDefinition {
    name: Cell<NameId>,
    local_name: Cell<NameId>,
    constructor: JsEdge,
    connected: JsEdge,
    disconnected: JsEdge,
    adopted: JsEdge,
    attribute_changed: JsEdge,
    observed: External<Box<[NameId]>>,
    disable_internals: Cell<bool>,
    disable_shadow: Cell<bool>,
    /// The callbacks it has (`reactions::Callback`'s bits): the engine's
    /// to say, as only it reads the edges.
    callbacks: Cell<u8>,
    /// Its construction stack's elements, and which of them are already
    /// constructed markers (the element let go of: `markers` alone tells).
    construction_stack: Member<HeapVector<Node>>,
    markers: External<Cell<Vec<bool>>>,
}

/// A promise `whenDefined(name)` gave before `name` was defined: the
/// number the engine keeps its resolver by.
#[derive(GcObject)]
pub struct WhenDefined {
    name: Cell<NameId>,
    id: Cell<u64>,
}

/// A definition's JS edges: the engine's to fill (`define`) and read.
pub struct DefinitionEdges<'gc> {
    pub constructor: &'gc JsEdge,
    pub connected: &'gc JsEdge,
    pub disconnected: &'gc JsEdge,
    pub adopted: &'gc JsEdge,
    pub attribute_changed: &'gc JsEdge,
}

/// What a definition is made with, besides its edges.
pub struct DefinitionInit {
    pub name: NameId,
    pub local_name: NameId,
    pub observed: Box<[NameId]>,
    pub disable_internals: bool,
    pub disable_shadow: bool,
}

impl<'gc> Cx<'_, 'gc> {
    /// `window`'s `customElements`: made the first time, the same after.
    pub fn custom_elements(&self, window: Gc<'gc, Window>) -> Gc<'gc, CustomElementRegistry> {
        if let Some(registry) = window.custom_elements.get(self.gc) {
            return registry;
        }
        let registry = self.gc.alloc_in(
            &sites::OTHER,
            CustomElementRegistryInit {
                window: Some(window),
                definitions: None,
                by_name: External::new(Cell::new(FxHashMap::default())),
                by_constructor: External::new(Cell::new(FxHashMap::default())),
                when_defined: None,
                running: Cell::new(false),
                candidates: None,
                candidates_by_name: External::new(Cell::new(FxHashMap::default())),
                wrapper: (),
            },
        );
        window.custom_elements.set(self.gc, Some(registry));
        registry
    }

    /// `window`'s registry, if it has been made (one with none defines
    /// nothing).
    pub fn custom_elements_if_made(
        &self,
        window: Gc<'gc, Window>,
    ) -> Option<Gc<'gc, CustomElementRegistry>> {
        window.custom_elements.get(self.gc)
    }

    /// The window whose registry it is.
    pub fn custom_elements_window(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
    ) -> Gc<'gc, Window> {
        registry.window.get(self.gc).expect("a registry's window")
    }

    pub(crate) fn custom_elements_wrapper_edge(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
    ) -> &'gc JsEdge {
        &registry.into_ref().wrapper
    }

    /// `registry`'s definitions, in the order they came.
    pub fn custom_element_definitions(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
    ) -> Vec<Gc<'gc, CustomElementDefinition>> {
        registry
            .definitions
            .get(self.gc)
            .map_or_else(Vec::new, |list| {
                (0..list.len())
                    .filter_map(|index| list.get(self.gc, index))
                    .collect()
            })
    }

    /// `registry`'s definition named `name`, if there is one.
    pub fn custom_element_definition(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
        name: NameId,
    ) -> Option<Gc<'gc, CustomElementDefinition>> {
        let list = registry.definitions.get(self.gc)?;
        let index = with_taken(&registry.by_name, |map| map.get(&name).copied())?;
        list.get(self.gc, usize::try_from(index).ok()?)
    }

    /// `registry`'s definitions whose constructors the engine numbered
    /// `number` (`set_definition_constructor_number`): one, mostly; the
    /// engine compares their constructors.
    pub fn custom_element_definitions_numbered(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
        number: u32,
    ) -> Vec<Gc<'gc, CustomElementDefinition>> {
        let Some(list) = registry.definitions.get(self.gc) else {
            return Vec::new();
        };
        with_taken(&registry.by_constructor, |map| map.get(&number).cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|index| list.get(self.gc, usize::try_from(index).ok()?))
            .collect()
    }

    /// The engine's number for `definition`'s constructor (V8's identity
    /// hash), set once it is stored.
    pub fn set_definition_constructor_number(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
        definition: Gc<'gc, CustomElementDefinition>,
        number: u32,
    ) {
        let index = self
            .definition_index(registry, definition)
            .expect("a definition of the registry");
        with_taken(&registry.by_constructor, |map| {
            map.entry(number).or_default().push(index)
        });
    }

    fn definition_index(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
        definition: Gc<'gc, CustomElementDefinition>,
    ) -> Option<u32> {
        let index = with_taken(&registry.by_name, |map| {
            map.get(&definition.name.get()).copied()
        })?;
        let list = registry.definitions.get(self.gc)?;
        list.get(self.gc, usize::try_from(index).ok()?)
            .is_some_and(|found| Gc::ptr_eq(found, definition))
            .then_some(index)
    }

    /// A new definition in `registry`, at the end: its edges empty, for the
    /// engine to fill.
    pub fn add_custom_element_definition(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
        init: DefinitionInit,
    ) -> Gc<'gc, CustomElementDefinition> {
        let definition = self.gc.alloc_in(
            &sites::OTHER,
            CustomElementDefinitionInit {
                name: Cell::new(init.name),
                local_name: Cell::new(init.local_name),
                constructor: (),
                connected: (),
                disconnected: (),
                adopted: (),
                attribute_changed: (),
                observed: External::new(init.observed),
                disable_internals: Cell::new(init.disable_internals),
                disable_shadow: Cell::new(init.disable_shadow),
                callbacks: Cell::new(0),
                construction_stack: None,
                markers: External::new(Cell::new(Vec::new())),
            },
        );
        let list = match registry.definitions.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                registry.definitions.set(self.gc, Some(list));
                list
            }
        };
        let index = u32::try_from(list.len()).expect("fewer than 2^32 definitions");
        list.push(self.gc, definition);
        with_taken(&registry.by_name, |map| map.insert(init.name, index));
        definition
    }

    /// `definition`'s JS edges.
    pub fn definition_edges(
        &self,
        definition: Gc<'gc, CustomElementDefinition>,
    ) -> DefinitionEdges<'gc> {
        let definition = definition.into_ref();
        DefinitionEdges {
            constructor: &definition.constructor,
            connected: &definition.connected,
            disconnected: &definition.disconnected,
            adopted: &definition.adopted,
            attribute_changed: &definition.attribute_changed,
        }
    }

    pub fn definition_name(&self, definition: Gc<'gc, CustomElementDefinition>) -> NameId {
        definition.name.get()
    }

    pub fn definition_local_name(&self, definition: Gc<'gc, CustomElementDefinition>) -> NameId {
        definition.local_name.get()
    }

    /// `definition`'s observed attributes (`observedAttributes`, as given).
    pub fn definition_observed_attributes(
        &self,
        definition: Gc<'gc, CustomElementDefinition>,
    ) -> &'gc [NameId] {
        definition.into_ref().observed.get()
    }

    /// Whether `definition`'s `disabledFeatures` have `internals`, and
    /// `shadow`.
    pub fn definition_disabled(
        &self,
        definition: Gc<'gc, CustomElementDefinition>,
    ) -> (bool, bool) {
        (
            definition.disable_internals.get(),
            definition.disable_shadow.get(),
        )
    }

    /// Which callbacks `definition` has (`Callback::bit`s), as the engine
    /// stored them.
    pub fn set_definition_callbacks(
        &self,
        definition: Gc<'gc, CustomElementDefinition>,
        callbacks: u8,
    ) {
        definition.callbacks.set(callbacks);
    }

    pub fn definition_has_callback(
        &self,
        definition: Gc<'gc, CustomElementDefinition>,
        callback: crate::Callback,
    ) -> bool {
        definition.callbacks.get() & callback.bit() != 0
    }

    /// Whether `element`'s state isn't uncustomized (one compare of
    /// `Node::scope`'s bits, as a subtree's walk reads them): undefined or
    /// custom, whose connection matters, or failed or precustomized.
    #[inline]
    pub(crate) fn may_have_reactions(&self, element: NodeRef<'gc>) -> bool {
        element.scope.get() & STATE_MASK != 0
    }

    /// Whether `element` is custom.
    #[inline]
    pub(crate) fn is_custom_element(&self, element: NodeRef<'gc>) -> bool {
        element.scope.get() & STATE_MASK == CustomElementState::Custom.bits()
    }

    /// HTML's "element definition is running" of `registry`.
    pub fn element_definition_is_running(&self, registry: Gc<'gc, CustomElementRegistry>) -> bool {
        registry.running.get()
    }

    pub fn set_element_definition_is_running(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
        running: bool,
    ) {
        registry.running.set(running);
    }

    /// A pending `whenDefined(name)` promise of `registry`: its number,
    /// the engine's to keep its resolver by.
    pub fn add_when_defined(&self, registry: Gc<'gc, CustomElementRegistry>, name: NameId) -> u64 {
        let id = self.new_promise_number();
        let pending = self.gc.alloc_in(
            &sites::OTHER,
            WhenDefinedInit {
                name: Cell::new(name),
                id: Cell::new(id),
            },
        );
        let list = match registry.when_defined.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                registry.when_defined.set(self.gc, Some(list));
                list
            }
        };
        list.push(self.gc, pending);
        id
    }

    /// `registry`'s pending `whenDefined(name)` promises' numbers, taken
    /// (the name was defined: the engine resolves them).
    pub fn take_when_defined(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
        name: NameId,
    ) -> Vec<u64> {
        let Some(list) = registry.when_defined.get(self.gc) else {
            return Vec::new();
        };
        let mut taken = Vec::new();
        let mut index = 0;
        while let Some(pending) = list.get(self.gc, index) {
            if pending.name.get() == name {
                taken.push(list.remove(self.gc, index).id.get());
            } else {
                index = index.saturating_add(1);
            }
        }
        taken
    }

    /// `registry`'s pending `whenDefined(name)` promise's number, if there
    /// is one.
    pub fn when_defined(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
        name: NameId,
    ) -> Option<u64> {
        let list = registry.when_defined.get(self.gc)?;
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .find(|pending| pending.name.get() == name)
            .map(|pending| pending.id.get())
    }

    /// A new number for a promise the engine keeps the resolver of
    /// (`whenDefined`'s, `decode()`'s, `wakeLock.request()`'s).
    pub fn new_promise_number(&self) -> u64 {
        let id = self
            .changes
            .promises_made
            .get()
            .checked_add(1)
            .expect("fewer than 2^64 promises");
        self.changes.promises_made.set(id);
        id
    }

    /// `name` as a custom element name's number, if it is a valid one
    /// (interned).
    pub fn custom_element_name(&self, name: &str) -> Option<NameId> {
        is_valid_custom_element_name(name).then(|| names::intern(name))
    }

    /// `element`'s custom element state.
    #[inline]
    pub fn custom_element_state(&self, element: NodeRef<'gc>) -> CustomElementState {
        CustomElementState::from_bits(element.scope.get())
    }

    /// `element`'s state set: a change of whether it is defined
    /// (`:defined`) is style's, as a focus's is.
    pub fn set_custom_element_state(&self, element: NodeRef<'gc>, state: CustomElementState) {
        debug_assert_eq!(self.kind(element), NodeKind::Element);
        let was = self.is_defined_element(element);
        element
            .scope
            .set((element.scope.get() & !STATE_MASK) | state.bits());
        if was != self.is_defined_element(element) {
            self.state_changed(element, crate::AttributeChanges::DEFINED);
        }
    }

    /// Whether `element` is defined (`:defined`: its custom element state
    /// uncustomized or custom).
    #[inline]
    pub fn is_defined_element(&self, element: NodeRef<'gc>) -> bool {
        let state = element.scope.get() & STATE_MASK;
        state == 0 || state == CustomElementState::Custom.bits()
    }

    /// Whether `element` is undefined (`Node::scope`'s bits: what a
    /// connected element's insertion reads).
    #[inline]
    pub(crate) fn is_undefined_custom_element(&self, element: NodeRef<'gc>) -> bool {
        element.scope.get() & STATE_MASK == UNDEFINED
    }

    /// `element`'s custom element definition (set as it is constructed or
    /// upgraded), if it has one.
    pub fn element_custom_definition(
        &self,
        element: NodeRef<'gc>,
    ) -> Option<Gc<'gc, CustomElementDefinition>> {
        element
            .data
            .get(self.gc)?
            .downcast::<ElementData>()?
            .custom_definition
            .get(self.gc)
    }

    pub fn set_element_custom_definition(
        &self,
        element: NodeRef<'gc>,
        definition: Gc<'gc, CustomElementDefinition>,
    ) {
        self.element_data(element)
            .custom_definition
            .set(self.gc, Some(definition));
    }

    /// `element`'s definition taken away (its upgrade threw).
    pub fn clear_element_custom_definition(&self, element: NodeRef<'gc>) {
        if let Some(data) = element
            .data
            .get(self.gc)
            .and_then(|data| data.downcast::<ElementData>())
        {
            data.custom_definition.set(self.gc, None);
        }
    }

    /// HTML's "look up a custom element definition" for an HTML element
    /// `local` of `document`: its window's registry's (none without a
    /// window or a registry, or for a name never interned: none defined).
    pub fn custom_element_definition_in(
        &self,
        document: NodeRef<'gc>,
        local: &str,
    ) -> Option<Gc<'gc, CustomElementDefinition>> {
        let window = self.window_of(document)?;
        let registry = self.custom_elements_if_made(window)?;
        self.custom_element_definition(registry, names::find(local)?)
    }

    /// HTML's "look up a custom element definition" for `element` (its
    /// node document's window's registry, by its local name, if it is an
    /// HTML element): autonomous ones only.
    pub fn custom_element_definition_for(
        &self,
        element: NodeRef<'gc>,
    ) -> Option<Gc<'gc, CustomElementDefinition>> {
        let name = self.element_name(element)?;
        if name.namespace != names::HTML_NAMESPACE || name.prefix != names::NONE {
            return None;
        }
        let window = self.window_of(self.document_of(element))?;
        let registry = self.custom_elements_if_made(window)?;
        self.custom_element_definition(registry, name.local)
    }

    /// The undefined elements among `root`'s shadow-including inclusive
    /// descendants, in shadow-including tree order, whose definitions
    /// there are (HTML's "try to upgrade" of each:
    /// `customElements.upgrade`), with them.
    pub fn upgradable_under(
        &self,
        root: NodeRef<'gc>,
    ) -> Vec<(NodeRef<'gc>, Gc<'gc, CustomElementDefinition>)> {
        let mut found = Vec::new();
        self.for_each_shadow_including_inclusive_descendant(root, |at, _| {
            if self.is_undefined_custom_element(at)
                && let Some(definition) = self.custom_element_definition_for(at)
            {
                found.push((at, definition));
            }
        });
        found
    }

    /// `element` pushed onto `definition`'s construction stack (an
    /// upgrade's).
    pub fn push_construction(
        &self,
        definition: Gc<'gc, CustomElementDefinition>,
        element: NodeRef<'gc>,
    ) {
        let stack = match definition.construction_stack.get(self.gc) {
            Some(stack) => stack,
            None => {
                let stack = HeapVector::new(self.gc, 1);
                definition.construction_stack.set(self.gc, Some(stack));
                stack
            }
        };
        stack.push(self.gc, element);
        with_taken(&definition.markers, |markers| markers.push(false));
    }

    /// The last of `definition`'s construction stack, if it isn't empty.
    pub fn construction_top(
        &self,
        definition: Gc<'gc, CustomElementDefinition>,
    ) -> Option<ConstructionTop<'gc>> {
        let stack = definition.construction_stack.get(self.gc)?;
        let last = stack.len().checked_sub(1)?;
        if with_taken(&definition.markers, |markers| markers.last().copied()).unwrap_or(false) {
            return Some(ConstructionTop::AlreadyConstructed);
        }
        stack.get(self.gc, last).map(ConstructionTop::Element)
    }

    /// The last of `definition`'s construction stack replaced by an
    /// already constructed marker.
    pub fn mark_construction_top(&self, definition: Gc<'gc, CustomElementDefinition>) {
        with_taken(&definition.markers, |markers| {
            if let Some(last) = markers.last_mut() {
                *last = true;
            }
        });
    }

    /// The last of `definition`'s construction stack popped.
    pub fn pop_construction(&self, definition: Gc<'gc, CustomElementDefinition>) {
        let Some(stack) = definition.construction_stack.get(self.gc) else {
            return;
        };
        if let Some(last) = stack.len().checked_sub(1) {
            stack.remove(self.gc, last);
            with_taken(&definition.markers, |markers| markers.pop());
        }
    }

    /// HTML's fragment parsing's custom elements, for a fragment no
    /// document holds (`createContextualFragment`'s): each undefined
    /// element of `root`'s subtree whose name its document's registry
    /// defines gets an upgrade reaction (HTML's "create an element" without
    /// the synchronous custom elements flag); the others wait to be
    /// connected.
    pub fn upgrade_defined_in(&self, root: NodeRef<'gc>) {
        let Some(window) = self.window_of(self.document_of(root)) else {
            return;
        };
        let registry = self.custom_elements(window);
        self.for_each_inclusive_descendant(root, |at| {
            if self.custom_element_state(at) != crate::CustomElementState::Undefined {
                return;
            }
            let definition = self
                .element_name(at)
                .and_then(|name| self.custom_element_definition(registry, name.local));
            if let Some(definition) = definition {
                self.enqueue_upgrade_reaction(at, definition);
            }
        });
    }

    /// HTML's "try to upgrade", for an undefined `element` just connected:
    /// an upgrade reaction if its name is defined in its document's
    /// registry; else a candidate of it.
    #[cold]
    #[inline(never)]
    pub(crate) fn try_to_upgrade(&self, element: NodeRef<'gc>) {
        let Some(window) = self.window_of(self.document_of(element)) else {
            return;
        };
        let registry = self.custom_elements(window);
        let Some(name) = self.element_name(element).map(|name| name.local) else {
            return;
        };
        if let Some(definition) = self.custom_element_definition(registry, name) {
            self.enqueue_upgrade_reaction(element, definition);
            return;
        }
        let lists = match registry.candidates.get(self.gc) {
            Some(lists) => lists,
            None => {
                let lists = HeapVector::new(self.gc, 1);
                registry.candidates.set(self.gc, Some(lists));
                lists
            }
        };
        let index = with_taken(&registry.candidates_by_name, |map| map.get(&name).copied());
        let candidates =
            match index.and_then(|index| lists.get(self.gc, usize::try_from(index).ok()?)) {
                Some(candidates) => candidates,
                None => {
                    let candidates = self.gc.alloc_in(
                        &sites::OTHER,
                        CandidateListInit {
                            list: Some(HeapVector::new(self.gc, 1)),
                            prune_at: Cell::new(16),
                        },
                    );
                    let index = u32::try_from(lists.len()).expect("fewer than 2^32 names");
                    lists.push(self.gc, candidates);
                    with_taken(&registry.candidates_by_name, |map| map.insert(name, index));
                    candidates
                }
            };
        let list = candidates
            .list
            .get(self.gc)
            .expect("a candidate list's list");
        let candidate = self.gc.alloc_in(
            &sites::OTHER,
            CandidateInit {
                element: Some(element),
            },
        );
        list.push(self.gc, candidate);
        if u32::try_from(list.len()).unwrap_or(u32::MAX) >= candidates.prune_at.get() {
            // Those dead let go of, the room doubled past the live ones.
            let mut index = 0;
            while let Some(candidate) = list.get(self.gc, index) {
                if candidate.element.get(self.gc).is_some() {
                    index = index.saturating_add(1);
                } else {
                    list.remove(self.gc, index);
                }
            }
            let live = u32::try_from(list.len()).unwrap_or(u32::MAX);
            candidates.prune_at.set(live.saturating_mul(2).max(16));
        }
    }

    /// `registry`'s upgrade candidates named `name`, taken, those still
    /// candidates (alive, connected, undefined, in its window's document)
    /// in shadow-including tree order (Blink's `CollectCandidates`, its
    /// `CustomElementUpgradeSorter`): `define`'s to upgrade.
    pub fn take_upgrade_candidates(
        &self,
        registry: Gc<'gc, CustomElementRegistry>,
        name: NameId,
    ) -> Vec<NodeRef<'gc>> {
        let Some(lists) = registry.candidates.get(self.gc) else {
            return Vec::new();
        };
        let Some(index) = with_taken(&registry.candidates_by_name, |map| map.remove(&name)) else {
            return Vec::new();
        };
        let Some(candidates) = lists.get(self.gc, usize::try_from(index).expect("a list's place"))
        else {
            return Vec::new();
        };
        let list = candidates
            .list
            .get(self.gc)
            .expect("a candidate list's list");
        let document = self.window_document(self.custom_elements_window(registry));
        let mut found: Vec<(Vec<usize>, NodeRef<'gc>)> = (0..list.len())
            .filter_map(|index| list.get(self.gc, index)?.element.get(self.gc))
            .filter(|element| {
                self.is_connected(*element)
                    && self.is_undefined_custom_element(*element)
                    && Gc::ptr_eq(self.document_of(*element), document)
            })
            .map(|element| (self.shadow_including_path(element), element))
            .collect();
        list.clear(self.gc);
        candidates.prune_at.set(16);
        found.sort_by(|a, b| a.0.cmp(&b.0));
        found.dedup_by(|a, b| Gc::ptr_eq(a.1, b.1));
        found.into_iter().map(|(_, element)| element).collect()
    }

    /// Where `node` is in its document, shadow-including: each ancestor's
    /// place among its parent's, a shadow root's before its host's
    /// children (0), from the top. Comparing two orders them.
    fn shadow_including_path(&self, node: NodeRef<'gc>) -> Vec<usize> {
        let mut path = Vec::new();
        let mut at = node;
        loop {
            if let Some(parent) = self.parent(at) {
                let mut place = 1usize;
                let mut sibling = self.previous_sibling(at);
                while let Some(before) = sibling {
                    place = place.saturating_add(1);
                    sibling = self.previous_sibling(before);
                }
                path.push(place);
                at = parent;
            } else if let Some(host) = self.shadow_host(at) {
                path.push(0);
                at = host;
            } else {
                break;
            }
        }
        path.reverse();
        path
    }
}

/// `f` given what `cell` holds (taken and put back: no borrow is held
/// across it).
fn with_taken<T: Default + Send + 'static, R>(
    cell: &External<Cell<T>>,
    f: impl FnOnce(&mut T) -> R,
) -> R {
    let cell = cell.get();
    let mut taken = cell.take();
    let result = f(&mut taken);
    cell.set(taken);
    result
}
