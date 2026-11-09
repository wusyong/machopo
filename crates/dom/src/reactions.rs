//! Custom element reactions (HTML's; Blink's `CustomElementReactionStack`,
//! `CustomElementReactionQueue`, `CEReactionsScope`): each element's own
//! reaction queue (in its data), the custom element reactions stack of
//! element queues, and the backup element queue.
//!
//! A `[CEReactions]` member's call enters a scope (`enter_ce_reactions`)
//! and leaves it (`leave_ce_reactions`); a queue is pushed only as the
//! first reaction is enqueued in a scope (Blink's lazy push), so a call
//! that enqueues none counts once each way. Left, its queue is the
//! engine's to invoke, out of the operation. An element enqueued with no
//! scope entered goes in the backup element queue, which the engine
//! processes in a microtask (`Dom::take_microtasks`).
//!
//! Its reactions are an element's upgrade (its definition) and its
//! definition's lifecycle callbacks (`connectedCallback`,
//! `disconnectedCallback`, `adoptedCallback`, `attributeChangedCallback`)
//! with their arguments: enqueued only for a custom element whose
//! definition has the callback (the engine says which it has:
//! `set_definition_callbacks`), `attributeChangedCallback` only for an
//! observed attribute.

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, LocalExternal, Member, Persistent};

use crate::{CustomElementDefinition, Cx, DomString, NameId, Node, NodeRef, names, sites};

/// The stack and the backup queue (`Dom`'s).
#[derive(Default)]
pub(crate) struct ReactionStack {
    /// How many scopes are entered.
    depth: Cell<u32>,
    /// The element queues pushed, each with its scope's depth, and the
    /// last's (0: none), what leaving a scope reads.
    queues: Cell<Vec<(u32, Persistent<HeapVector<Node>>)>>,
    top: Cell<u32>,
    /// The queues of the scopes left, for the engine to invoke (innermost
    /// last): a scope's leaving gives a `bool`, a word the call's
    /// operation returns beside its result, not the queue's root.
    left: Cell<Vec<Persistent<HeapVector<Node>>>>,
    /// The backup element queue, once it has had elements; whether its
    /// processing is queued (HTML's "processing the backup element queue"
    /// flag); and whether the engine is yet to queue the microtask.
    backup: Cell<Option<Persistent<HeapVector<Node>>>>,
    backup_queued: Cell<bool>,
    /// HTML's "active custom element constructor map": the definitions
    /// whose constructors run for an upgrade or a synchronous creation,
    /// innermost last, which `HTMLElement`'s constructor finds before the
    /// current realm's registry (a constructor defined in another realm's
    /// registry). A stack: a constructor run again inside itself, by
    /// another registry's definition, is that one's until it returns.
    active_constructors: Cell<Vec<Persistent<CustomElementDefinition>>>,
}

/// An attribute change's old and new values (`attributeChangedCallback`'s).
type OldAndNew = (Option<DomString>, Option<DomString>);

/// One reaction of an element's queue: an upgrade with its definition,
/// or one of its definition's callbacks (`Callback`'s number) with its
/// arguments: `adoptedCallback`'s documents, `attributeChangedCallback`'s
/// name, namespace and values.
#[derive(GcObject)]
pub struct Reaction {
    kind: Cell<u8>,
    definition: Member<CustomElementDefinition>,
    old_document: Member<Node>,
    new_document: Member<Node>,
    local: Cell<NameId>,
    namespace: Cell<NameId>,
    values: LocalExternal<Option<Box<OldAndNew>>>,
}

/// A lifecycle callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Callback {
    Connected,
    Disconnected,
    Adopted,
    AttributeChanged,
}

impl Callback {
    /// Its bit among a definition's callbacks, and a reaction's kind.
    pub const fn bit(self) -> u8 {
        match self {
            Self::Connected => 1 << 0,
            Self::Disconnected => 1 << 1,
            Self::Adopted => 1 << 2,
            Self::AttributeChanged => 1 << 3,
        }
    }
}

/// An upgrade reaction's kind (no callback's bit).
const UPGRADE: u8 = 0;

/// A reaction taken off an element's queue.
#[derive(Clone)]
pub enum ReactionRef<'gc> {
    Upgrade(Gc<'gc, CustomElementDefinition>),
    Callback(Gc<'gc, CustomElementDefinition>, CallbackArgs<'gc>),
}

/// A callback's reaction's arguments.
#[derive(Clone)]
pub enum CallbackArgs<'gc> {
    Connected,
    Disconnected,
    /// The old document and the new.
    Adopted(NodeRef<'gc>, NodeRef<'gc>),
    /// The attribute's local name, old value, new value and namespace
    /// (none: no namespace).
    AttributeChanged {
        local: NameId,
        old: Option<DomString>,
        new: Option<DomString>,
        namespace: Option<NameId>,
    },
}

/// A `[CEReactions]` member's scope in its operation (Blink's
/// `CEReactionsScope`): entered as it is made, left by `leave` or, if a
/// step fails first, as it drops.
pub struct CeReactionsScope<'c, 'a, 'gc> {
    cx: &'c Cx<'a, 'gc>,
    left: bool,
}

impl<'c, 'a, 'gc> CeReactionsScope<'c, 'a, 'gc> {
    #[inline]
    pub fn enter(cx: &'c Cx<'a, 'gc>) -> Self {
        cx.enter_ce_reactions();
        Self { cx, left: false }
    }

    /// Left: whether it left an element queue (`Cx::leave_ce_reactions`).
    #[inline]
    pub fn leave(mut self) -> bool {
        self.left = true;
        self.cx.leave_ce_reactions()
    }
}

impl Drop for CeReactionsScope<'_, '_, '_> {
    #[inline]
    fn drop(&mut self) {
        if !self.left {
            self.cx.leave_ce_reactions();
        }
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// A `[CEReactions]` member's scope entered.
    #[inline]
    pub fn enter_ce_reactions(&self) {
        let stack = &self.changes.reactions;
        stack.depth.set(stack.depth.get().saturating_add(1));
    }

    /// The innermost scope left: whether a reaction was enqueued in it,
    /// its element queue then the engine's to invoke after the operation
    /// (`take_left_element_queue`).
    #[inline]
    pub fn leave_ce_reactions(&self) -> bool {
        let stack = &self.changes.reactions;
        let depth = stack.depth.get();
        stack.depth.set(depth.saturating_sub(1));
        if stack.top.get() != depth {
            return false;
        }
        self.pop_element_queue();
        true
    }

    #[cold]
    #[inline(never)]
    fn pop_element_queue(&self) {
        let stack = &self.changes.reactions;
        let mut queues = stack.queues.take();
        let (_, popped) = queues.pop().expect("the scope's queue");
        stack.top.set(queues.last().map_or(0, |(at, _)| *at));
        stack.queues.set(queues);
        let mut left = stack.left.take();
        left.push(popped);
        stack.left.set(left);
    }

    /// The element queue of the scope last left (`leave_ce_reactions`
    /// gave true), taken.
    pub fn take_left_element_queue(&self) -> Option<Persistent<HeapVector<Node>>> {
        let stack = &self.changes.reactions;
        let mut left = stack.left.take();
        let taken = left.pop();
        stack.left.set(left);
        taken
    }

    /// `definition`'s constructor is to run (an upgrade, a synchronous
    /// creation): `HTMLElement`'s constructor finds it first
    /// (`active_constructor`) until `pop_active_constructor`.
    pub fn push_active_constructor(&self, definition: Gc<'gc, CustomElementDefinition>) {
        let stack = &self.changes.reactions;
        let mut active = stack.active_constructors.take();
        active.push(Persistent::new(self.gc, definition));
        stack.active_constructors.set(active);
    }

    /// The constructor last pushed has returned.
    pub fn pop_active_constructor(&self) {
        let stack = &self.changes.reactions;
        let mut active = stack.active_constructors.take();
        active.pop();
        stack.active_constructors.set(active);
    }

    /// The innermost running definition `is` says is `new.target`'s.
    pub fn active_constructor(
        &self,
        mut is: impl FnMut(Gc<'gc, CustomElementDefinition>) -> bool,
    ) -> Option<Gc<'gc, CustomElementDefinition>> {
        let stack = &self.changes.reactions;
        let active = stack.active_constructors.take();
        let found = active
            .iter()
            .rev()
            .map(|definition| definition.get(self.gc))
            .find(|definition| is(*definition));
        stack.active_constructors.set(active);
        found
    }

    /// The first element of `queue`, taken.
    pub fn take_from_element_queue(
        &self,
        queue: &Persistent<HeapVector<Node>>,
    ) -> Option<NodeRef<'gc>> {
        let queue = queue.get(self.gc);
        if queue.is_empty() {
            return None;
        }
        Some(queue.remove(self.gc, 0))
    }

    /// HTML's "enqueue a custom element upgrade reaction": `definition`'s
    /// upgrade on `element`'s reaction queue, `element` on the appropriate
    /// element queue.
    pub fn enqueue_upgrade_reaction(
        &self,
        element: NodeRef<'gc>,
        definition: Gc<'gc, CustomElementDefinition>,
    ) {
        let reaction = self.gc.alloc_in(
            &sites::OTHER,
            ReactionInit {
                kind: Cell::new(UPGRADE),
                definition: Some(definition),
                old_document: None,
                new_document: None,
                local: Cell::new(names::NONE),
                namespace: Cell::new(names::NONE),
                values: LocalExternal::new(None),
            },
        );
        self.enqueue_reaction(element, reaction);
    }

    /// HTML's "enqueue a custom element callback reaction": `element`'s
    /// definition's `callback` with `args`, if the definition has it (and,
    /// for `attributeChangedCallback`, observes the attribute);
    /// `element` on the appropriate element queue.
    pub fn enqueue_callback_reaction(&self, element: NodeRef<'gc>, args: CallbackArgs<'gc>) {
        let Some(definition) = self.element_custom_definition(element) else {
            return;
        };
        let callback = match &args {
            CallbackArgs::Connected => Callback::Connected,
            CallbackArgs::Disconnected => Callback::Disconnected,
            CallbackArgs::Adopted(..) => Callback::Adopted,
            CallbackArgs::AttributeChanged { .. } => Callback::AttributeChanged,
        };
        if !self.definition_has_callback(definition, callback) {
            return;
        }
        let (mut old_document, mut new_document, mut local, mut namespace, mut values) =
            (None, None, names::NONE, names::NONE, None);
        match args {
            CallbackArgs::Connected | CallbackArgs::Disconnected => {}
            CallbackArgs::Adopted(old, new) => {
                (old_document, new_document) = (Some(old), Some(new));
            }
            CallbackArgs::AttributeChanged {
                local: name,
                old,
                new,
                namespace: space,
            } => {
                if !self
                    .definition_observed_attributes(definition)
                    .contains(&name)
                {
                    return;
                }
                (local, namespace, values) = (
                    name,
                    space.unwrap_or(names::NONE),
                    Some(Box::new((old, new))),
                );
            }
        }
        let reaction = self.gc.alloc_in(
            &sites::OTHER,
            ReactionInit {
                kind: Cell::new(callback.bit()),
                definition: Some(definition),
                old_document,
                new_document,
                local: Cell::new(local),
                namespace: Cell::new(namespace),
                values: LocalExternal::new(values),
            },
        );
        self.enqueue_reaction(element, reaction);
    }

    /// `element`'s reaction queue emptied (its upgrade threw).
    pub fn clear_element_reactions(&self, element: NodeRef<'gc>) {
        if let Some(queue) = element
            .data
            .get(self.gc)
            .and_then(|data| data.downcast::<crate::ElementData>())
            .and_then(|data| data.reactions.get(self.gc))
        {
            queue.clear(self.gc);
        }
    }

    fn enqueue_reaction(&self, element: NodeRef<'gc>, reaction: Gc<'gc, Reaction>) {
        let data = self.element_data(element);
        let queue = match data.reactions.get(self.gc) {
            Some(queue) => queue,
            None => {
                let queue = HeapVector::new(self.gc, 1);
                data.reactions.set(self.gc, Some(queue));
                queue
            }
        };
        queue.push(self.gc, reaction);
        self.enqueue_element(element);
    }

    /// The first reaction of `element`'s queue, taken.
    pub fn take_element_reaction(&self, element: NodeRef<'gc>) -> Option<ReactionRef<'gc>> {
        let queue = element
            .data
            .get(self.gc)?
            .downcast::<crate::ElementData>()?
            .reactions
            .get(self.gc)?;
        if queue.is_empty() {
            return None;
        }
        let reaction = queue.remove(self.gc, 0);
        let definition = reaction
            .definition
            .get(self.gc)
            .expect("a reaction's definition");
        let args = match reaction.kind.get() {
            UPGRADE => return Some(ReactionRef::Upgrade(definition)),
            kind if kind == Callback::Connected.bit() => CallbackArgs::Connected,
            kind if kind == Callback::Disconnected.bit() => CallbackArgs::Disconnected,
            kind if kind == Callback::Adopted.bit() => CallbackArgs::Adopted(
                reaction
                    .old_document
                    .get(self.gc)
                    .expect("an adoption's old document"),
                reaction
                    .new_document
                    .get(self.gc)
                    .expect("an adoption's new document"),
            ),
            _ => {
                let (old, new) = reaction
                    .into_ref()
                    .values
                    .get()
                    .as_deref()
                    .cloned()
                    .unwrap_or_default();
                CallbackArgs::AttributeChanged {
                    local: reaction.local.get(),
                    old,
                    new,
                    namespace: Some(reaction.namespace.get()).filter(|space| *space != names::NONE),
                }
            }
        };
        Some(ReactionRef::Callback(definition, args))
    }

    /// A custom element's (`Node::scope`'s state bits not uncustomized:
    /// one compare, as a connected subtree's walk reads them) connection:
    /// a custom one's `connectedCallback`, an undefined one's try to
    /// upgrade.
    #[cold]
    #[inline(never)]
    pub(crate) fn custom_element_connected(&self, element: NodeRef<'gc>) {
        match self.custom_element_state(element) {
            crate::CustomElementState::Custom => {
                self.enqueue_callback_reaction(element, CallbackArgs::Connected)
            }
            crate::CustomElementState::Undefined => self.try_to_upgrade(element),
            _ => {}
        }
    }

    /// A custom element taken out of a document: its `disconnectedCallback`.
    #[cold]
    #[inline(never)]
    pub(crate) fn custom_element_disconnected(&self, element: NodeRef<'gc>) {
        if self.custom_element_state(element) == crate::CustomElementState::Custom {
            self.enqueue_callback_reaction(element, CallbackArgs::Disconnected);
        }
    }

    /// A custom element's attribute `name` changed from `old` to `new`:
    /// its `attributeChangedCallback`.
    #[cold]
    #[inline(never)]
    pub(crate) fn custom_element_attribute_changed(
        &self,
        element: NodeRef<'gc>,
        name: crate::QualifiedName,
        old: Option<&str>,
        new: Option<&str>,
    ) {
        let ids = name.ids();
        self.enqueue_callback_reaction(
            element,
            CallbackArgs::AttributeChanged {
                local: ids.local,
                old: old.map(DomString::new),
                new: new.map(DomString::new),
                namespace: Some(ids.namespace).filter(|space| *space != names::NONE),
            },
        );
    }

    /// HTML's "enqueue an element on the appropriate element queue": the
    /// current element queue (pushed now if the innermost scope has none),
    /// or with no scope the backup one, its processing queued once.
    fn enqueue_element(&self, element: NodeRef<'gc>) {
        let stack = &self.changes.reactions;
        let depth = stack.depth.get();
        if depth == 0 {
            let backup = match stack.backup.take() {
                Some(backup) => backup,
                None => Persistent::new(self.gc, HeapVector::new(self.gc, 1)),
            };
            backup.get(self.gc).push(self.gc, element);
            stack.backup.set(Some(backup));
            if !stack.backup_queued.replace(true) {
                self.microtask_to_queue(crate::Microtasks::REACTIONS);
            }
            return;
        }
        let mut queues = stack.queues.take();
        if stack.top.get() != depth {
            queues.push((depth, Persistent::new(self.gc, HeapVector::new(self.gc, 1))));
            stack.top.set(depth);
        }
        let (_, queue) = queues.last().expect("a queue pushed");
        queue.get(self.gc).push(self.gc, element);
        stack.queues.set(queues);
    }

    /// The first element of the backup element queue, taken; none left:
    /// its processing done (the flag unset).
    pub fn take_from_backup_element_queue(&self) -> Option<NodeRef<'gc>> {
        let stack = &self.changes.reactions;
        let backup = stack.backup.take();
        let taken = backup.as_ref().and_then(|backup| {
            let queue = backup.get(self.gc);
            (!queue.is_empty()).then(|| queue.remove(self.gc, 0))
        });
        stack.backup.set(backup);
        if taken.is_none() {
            stack.backup_queued.set(false);
        }
        taken
    }
}
