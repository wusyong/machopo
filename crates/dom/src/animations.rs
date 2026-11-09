//! Each element's CSS animations and transitions (Blink's
//! `ElementAnimations` on its `ElementRareData`), and each document's
//! timeline and the events its animations queued (Blink's
//! `DocumentAnimations`, `DocumentTimeline`).
//!
//! An element's are in a heap object on its rare data, made at its first:
//! the element's own and its pseudo-elements', each an
//! `animation::AnimatedStyle` (its animations and the style the cascade
//! last made, which their values are applied to). They live as long as the
//! element does. Its document keeps the elements that have some, strongly
//! (Blink's timeline keeps the animations it updates): each frame moves
//! them on, cancels those of an element no longer in it, and lets go of
//! those with none left.
//!
//! Style writes them as it styles an element (`style_change_animations`):
//! what animations run is lifecycle output, as computed styles are. The
//! isolate's count of animations (their order) and the subtrees removed
//! from their trees since they were styled, whose styles are from before
//! (css-transitions 1: no before-change style), are the DOM's.

use std::cell::{Cell, RefCell};

use animation::{AnimatedStyle, Animation, AnimationEvent};
use computed_style::PseudoId;
use elise::{External, Gc, GcObject, HeapVector, LocalExternal};

use crate::{Cx, DomString, Event, NodeId, NodeIdHashSet, NodeRef, sites};

/// An element's animations and its pseudo-elements' (by [`slot`]), and
/// whether they moved on since it was last styled.
#[derive(GcObject)]
pub struct ElementAnimationsData {
    targets: External<RefCell<[Option<AnimatedStyle>; 4]>>,
    /// They moved on since style last looked (`animations_moved_on`).
    moved_on: Cell<bool>,
    /// It is in its document's list (`DocumentData::animated`).
    listed: Cell<bool>,
}

/// Where a target's animations are in [`ElementAnimationsData`]: the
/// element's own, then its `::before`'s, `::after`'s, `::marker`'s.
fn slot(pseudo: Option<PseudoId>) -> usize {
    match pseudo {
        None => 0,
        Some(PseudoId::Before) => 1,
        Some(PseudoId::After) => 2,
        Some(PseudoId::Marker) => 3,
    }
}

const PSEUDOS: [Option<PseudoId>; 4] = [
    None,
    Some(PseudoId::Before),
    Some(PseudoId::After),
    Some(PseudoId::Marker),
];

/// An event an animation sent, waiting for the rendering update to
/// dispatch it at its target.
#[derive(Debug, Clone)]
pub struct PendingAnimationEvent {
    /// None for an animation off any element (its playback's notices).
    pub element: Option<NodeId>,
    pub pseudo: Option<PseudoId>,
    pub event: AnimationEvent,
}

/// Where an animation is.
#[derive(Debug, Clone, Copy)]
pub enum AnimationPlace<'gc> {
    /// On an element, or one of its pseudo-elements.
    Target(NodeRef<'gc>, Option<PseudoId>),
    /// Off any: no target, or style's that left its element.
    Detached,
}

/// A `TransitionEvent`'s or an `AnimationEvent`'s data: the property's
/// or the animation's name, the time it ran, its pseudo-element (`::before`,
/// or empty for the element).
#[derive(GcObject)]
pub struct AnimationEventData {
    transition: Cell<bool>,
    name: LocalExternal<DomString>,
    elapsed_time: Cell<f64>,
    pseudo_element: LocalExternal<DomString>,
}

/// What [`AnimationEventData`] has: whether it is a `TransitionEvent`'s,
/// and its members.
#[derive(Debug, Clone, Copy)]
pub struct AnimationEventFields<'a> {
    pub transition: bool,
    pub name: &'a DomString,
    pub elapsed_time: f64,
    pub pseudo_element: &'a DomString,
}

/// The isolate's: the last animation's number, whether an element has had
/// animations (removals are recorded from then on), and the roots of the
/// subtrees removed from their trees since (their elements' styles are
/// from before: a transition doesn't start from them, and their
/// animations stop when they are styled again).
#[derive(Default)]
pub(crate) struct AnimationState {
    pub(crate) order: Cell<u64>,
    seen: Cell<bool>,
    removed: RefCell<NodeIdHashSet>,
    /// The animations' objects with notices for their promises
    /// (`push_animation_notice`).
    pub(crate) noticed: Cell<Option<elise::Persistent<HeapVector<crate::EventTargetImpl>>>>,
}

/// An effect's target: an element, and a pseudo-element of it if any.
type EffectTarget<'gc> = (NodeRef<'gc>, Option<PseudoId>);

impl<'gc> Cx<'_, 'gc> {
    /// `element`'s data, if it ever had animations.
    fn element_animations(&self, element: NodeRef<'gc>) -> Option<Gc<'gc, ElementAnimationsData>> {
        self.node_rare(element, false)?.animations.get(self.gc)
    }

    /// The style change event of `element` (or its pseudo-element
    /// `pseudo`): `change` given its animations (none if it has none; none
    /// when it is done with them) and the isolate's count of animations;
    /// they are kept on it, and it in its document's list, if it has
    /// some.
    pub fn style_change_animations<R>(
        &self,
        element: NodeRef<'gc>,
        pseudo: Option<PseudoId>,
        change: impl FnOnce(&mut Option<AnimatedStyle>, &mut u64) -> R,
    ) -> R {
        let state = &self.changes.animations;
        let mut order = state.order.get();
        let existing = self.element_animations(element);
        let (result, made) = match existing {
            Some(data) => {
                let result = change(&mut data.targets.borrow_mut()[slot(pseudo)], &mut order);
                (result, None)
            }
            None => {
                let mut target = None;
                let result = change(&mut target, &mut order);
                (result, target)
            }
        };
        state.order.set(order);
        let data = match (existing, made) {
            (Some(data), _) => data,
            (None, None) => return result,
            (None, Some(target)) => {
                let mut targets: [Option<AnimatedStyle>; 4] = Default::default();
                targets[slot(pseudo)] = Some(target);
                let data = self.gc.alloc_in(
                    &crate::sites::OTHER,
                    ElementAnimationsDataInit {
                        targets: External::new(RefCell::new(targets)),
                        moved_on: Cell::new(false),
                        listed: Cell::new(false),
                    },
                );
                self.node_rare(element, true)
                    .expect("an element's rare data")
                    .animations
                    .set(self.gc, Some(data));
                data
            }
        };
        if !data.listed.get() && data.targets.borrow().iter().any(Option::is_some) {
            state.seen.set(true);
            let document = self.ensure_document_data(self.document_of(element));
            let list = match document.animated.get(self.gc) {
                Some(list) => list,
                None => {
                    let list = HeapVector::new(self.gc, 1);
                    document.animated.set(self.gc, Some(list));
                    list
                }
            };
            list.push(self.gc, element);
            data.listed.set(true);
        }
        result
    }

    /// `element`'s animations and its pseudo-elements' (`None` for a
    /// target without), read by `read`; `None` if it never had any.
    pub fn read_animations<R>(
        &self,
        element: NodeRef<'gc>,
        read: impl FnOnce(&[Option<AnimatedStyle>; 4]) -> R,
    ) -> Option<R> {
        let data = self.element_animations(element)?;
        Some(read(&data.targets.borrow()))
    }

    /// `element`'s animations changed (script moved one to or off it): it
    /// is styled again, not just sampled (one that has none left has no
    /// base style to sample them on).
    pub fn animations_changed(&self, element: NodeRef<'gc>) {
        self.set_needs_style_recalc(element);
    }

    /// Whether an element has had animations: until then, the style phase
    /// needn't look for what they need.
    pub fn has_seen_animations(&self) -> bool {
        self.changes.animations.seen.get()
    }

    /// Whether `element`'s animations moved on since style last looked
    /// (its style is their values at an earlier time); cleared.
    pub fn take_animations_moved_on(&self, element: NodeRef<'gc>) -> bool {
        self.element_animations(element)
            .is_some_and(|data| data.moved_on.replace(false))
    }

    /// `document`'s timeline's time, in milliseconds: as of its last frame
    /// (0 before one).
    pub fn animation_time(&self, document: NodeRef<'gc>) -> f64 {
        self.timeline_time(document).unwrap_or(0.0)
    }

    /// `document`'s timeline's time, if a frame (or script, before one:
    /// `start_timeline`) gave it one.
    pub fn timeline_time(&self, document: NodeRef<'gc>) -> Option<f64> {
        self.document_data(document)?.animation_time.get()
    }

    /// `document`'s timeline's time before its first frame: `now`, kept
    /// till that frame (so that its animations agree; Blink's animation
    /// clock is "now" until a frame sets it).
    pub fn start_timeline(&self, document: NodeRef<'gc>, now: f64) -> f64 {
        let data = self.ensure_document_data(document);
        match data.animation_time.get() {
            Some(time) => time,
            None => {
                data.animation_time.set(Some(now));
                now
            }
        }
    }

    /// `sent`, the events of `element`'s (or its pseudo-element's)
    /// animations, queued in its document for the rendering update.
    pub fn queue_animation_events(
        &self,
        element: NodeRef<'gc>,
        pseudo: Option<PseudoId>,
        sent: Vec<AnimationEvent>,
    ) {
        if sent.is_empty() {
            return;
        }
        let document = self.document_of(element);
        self.ensure_document_data(document);
        self.queue_events_in(document, element, pseudo, sent);
    }

    /// The events `document`'s animations queued, taken.
    pub fn take_animation_events(&self, document: NodeRef<'gc>) -> Vec<PendingAnimationEvent> {
        self.document_data(document)
            .map(|data| std::mem::take(&mut *data.animation_events.borrow_mut()))
            .unwrap_or_default()
    }

    /// Whether `node` was removed from its tree since it was styled (its
    /// style is from before): the root of such a subtree.
    pub fn removed_since_styled(&self, node: NodeRef<'gc>) -> bool {
        let removed = self.changes.animations.removed.borrow();
        !removed.is_empty() && removed.contains(&self.node_id(node))
    }

    /// The removed subtrees whose roots are `roots` were styled again in
    /// their trees: their styles are new.
    pub fn forget_removed(&self, roots: &[NodeId]) {
        let mut removed = self.changes.animations.removed.borrow_mut();
        for root in roots {
            removed.remove(root);
        }
    }

    /// `node` was removed from its tree: once an element has had
    /// animations, its subtree's styles are from before. Only an element's
    /// is noted: style walks elements, so a text's would never be
    /// forgotten (none has a style or animations).
    pub(crate) fn animations_removed(&self, node: NodeRef<'gc>) {
        let state = &self.changes.animations;
        if state.seen.get() && self.kind(node) == crate::NodeKind::Element {
            state.removed.borrow_mut().insert(self.node_id(node));
        }
    }

    /// `document`'s animations at `now`, its timeline's time at a frame
    /// (Blink's `DocumentAnimations::UpdateAnimationTimingForAnimationFrame`):
    /// those of an element no longer in it are canceled; the others move
    /// on, their events queued, their elements styled again with their
    /// values; then those replaced are removed (web-animations 1). An
    /// element with none left leaves the list. Returns whether any ran:
    /// the next frame moves them on again.
    pub fn tick_animations(&self, document: NodeRef<'gc>, now: f64) -> bool {
        let Some(data) = self.document_data(document) else {
            return false;
        };
        data.animation_time.set(Some(now));
        // The removed subtrees collected.
        self.changes
            .animations
            .removed
            .borrow_mut()
            .retain(|id| self.node_by_id(*id).is_some());
        // Script's objects alive: what they hold is kept.
        let held = self.animation_object_ids(document);
        let mut running = self.tick_detached(document, now, &held);
        let Some(list) = data.animated.get(self.gc) else {
            return running;
        };
        // Backwards: an element done with them is taken out as it goes.
        for index in (0..list.len()).rev() {
            let Some(element) = list.get(self.gc, index) else {
                continue;
            };
            let animations = self
                .element_animations(element)
                .expect("a listed element has animations");
            let here =
                self.is_connected(element) && Gc::ptr_eq(self.document_of(element), document);
            let mut moved_on = false;
            let mut removed = Vec::new();
            {
                let mut targets = animations.targets.borrow_mut();
                for (pseudo, target) in PSEUDOS.into_iter().zip(targets.iter_mut()) {
                    let mut sent = Vec::new();
                    match target {
                        // css-transitions 1 and css-animations 1: canceled,
                        // with their events.
                        Some(entry) if !here => {
                            entry.animations.cancel_all(now, &mut sent);
                            removed.extend(entry.animations.take_removed());
                            *target = None;
                        }
                        Some(entry) => {
                            // Those done since the last frame that nothing
                            // can reach any more (no script holds one yet:
                            // neither pending, nor playing, nor filling) go
                            // first; this frame
                            // samples the rest.
                            entry.animations.forget(|animation| {
                                held.contains(&animation.id)
                                    || animation.playback.is_pending()
                                    || animation.is_reachable(now)
                            });
                            if entry.animations.is_empty() {
                                *target = None;
                                continue;
                            }
                            moved_on |= entry.animations.is_running();
                            entry.animations.tick(now, &mut sent);
                        }
                        None => {}
                    }
                    self.queue_events_in(document, element, pseudo, sent);
                }
            }
            self.keep_detached_animations(document, removed);
            if moved_on {
                running = true;
                animations.moved_on.set(true);
                self.mark_style_path(element);
            }
            if animations.targets.borrow().iter().all(Option::is_none) {
                animations.listed.set(false);
                list.remove(self.gc, index);
            }
        }
        // Then, every one moved on, those others replaced are removed.
        for index in 0..list.len() {
            let Some(element) = list.get(self.gc, index) else {
                continue;
            };
            let Some(animations) = self.element_animations(element) else {
                continue;
            };
            let mut targets = animations.targets.borrow_mut();
            for (pseudo, target) in PSEUDOS.into_iter().zip(targets.iter_mut()) {
                if let Some(entry) = target {
                    let mut sent = Vec::new();
                    entry.animations.remove_replaced(now, &mut sent);
                    self.queue_events_in(document, element, pseudo, sent);
                }
            }
        }
        running
    }

    /// The animations of `document` the last rendering started (or paused)
    /// start at its timeline's time: once a frame shows them (Blink's
    /// `PendingAnimations::Update`). Their events wait for the next update
    /// of animations. Whether any started: the next frame moves them on
    /// (a paused one's `animationstart` too), as Blink schedules one.
    pub fn start_pending_animations(&self, document: NodeRef<'gc>) -> bool {
        let Some(data) = self.document_data(document) else {
            return false;
        };
        let now = self.animation_time(document);
        let mut started = false;
        // Those off any element, which script plays.
        let mut sent = Vec::new();
        for animation in data.detached_animations.borrow_mut().iter_mut() {
            started |= animation.playback.is_pending();
            animation.commit_pending_detached(now, &mut sent);
        }
        self.queue_detached_events(document, sent);
        let Some(list) = data.animated.get(self.gc) else {
            return started;
        };
        for index in 0..list.len() {
            let Some(element) = list.get(self.gc, index) else {
                continue;
            };
            let Some(animations) = self.element_animations(element) else {
                continue;
            };
            let mut targets = animations.targets.borrow_mut();
            for (pseudo, target) in PSEUDOS.into_iter().zip(targets.iter_mut()) {
                if let Some(entry) = target {
                    let mut sent = Vec::new();
                    started |= entry.animations.commit_pending(now, &mut sent);
                    self.queue_events_in(document, element, pseudo, sent);
                }
            }
        }
        started
    }

    /// Whether `document` has elements with animations, or events of
    /// theirs to send: its update of animations has something to do.
    pub fn has_animations(&self, document: NodeRef<'gc>) -> bool {
        self.document_data(document).is_some_and(|data| {
            !data.animation_events.borrow().is_empty()
                || !data.detached_animations.borrow().is_empty()
                || data
                    .animated
                    .get(self.gc)
                    .is_some_and(|list| !list.is_empty())
        })
    }

    /// Whether `document` has animations that run, or events of theirs to
    /// send: frames are wanted while it does.
    pub fn animations_want_frames(&self, document: NodeRef<'gc>) -> bool {
        let Some(data) = self.document_data(document) else {
            return false;
        };
        if !data.animation_events.borrow().is_empty()
            || data
                .detached_animations
                .borrow()
                .iter()
                .any(Animation::is_running)
        {
            return true;
        }
        let Some(list) = data.animated.get(self.gc) else {
            return false;
        };
        (0..list.len()).any(|index| {
            list.get(self.gc, index)
                .and_then(|element| self.element_animations(element))
                .is_some_and(|animations| {
                    animations
                        .targets
                        .borrow()
                        .iter()
                        .flatten()
                        .any(|target| target.animations.is_running())
                })
        })
    }

    /// A new number for an animation or an effect (the isolate's count: its
    /// composite order, and a disassociated one's place in the global
    /// animation list).
    pub fn next_animation_number(&self) -> u64 {
        let order = &self.changes.animations.order;
        let next = order
            .get()
            .checked_add(1)
            .expect("fewer than 2^64 animations");
        order.set(next);
        next
    }

    /// The numbers of `document`'s animations script's objects stand for.
    fn animation_object_ids(&self, document: NodeRef<'gc>) -> std::collections::HashSet<u64> {
        self.prune_animation_objects(document, |_| true)
            .into_iter()
            .collect()
    }

    /// `removed`, animations that left their element (canceled, replaced,
    /// done), kept off any element while script holds their objects.
    pub fn keep_detached_animations(&self, document: NodeRef<'gc>, removed: Vec<Animation>) {
        let kept: Vec<Animation> = removed
            .into_iter()
            .filter(|animation| {
                self.existing_animation_object(document, animation.id)
                    .is_some()
            })
            .collect();
        if kept.is_empty() {
            return;
        }
        self.ensure_document_data(document)
            .detached_animations
            .borrow_mut()
            .extend(kept);
    }

    /// `document`'s animations off any element at `now`: they move on as
    /// script plays them, their notices queued; those no object `held`
    /// stands for go. Whether any runs.
    fn tick_detached(
        &self,
        document: NodeRef<'gc>,
        now: f64,
        held: &std::collections::HashSet<u64>,
    ) -> bool {
        let Some(data) = self.document_data(document) else {
            return false;
        };
        let mut sent = Vec::new();
        let mut running = false;
        {
            let mut detached = data.detached_animations.borrow_mut();
            detached.retain(|animation| held.contains(&animation.id));
            for animation in detached.iter_mut() {
                running |= animation.is_running();
                animation.tick_detached(now, &mut sent);
            }
        }
        self.queue_detached_events(document, sent);
        running
    }

    /// Where `document`'s animation numbered `id` is: on an element (or a
    /// pseudo-element of it; `hint`'s looked at first), or off any.
    fn locate(
        &self,
        document: NodeRef<'gc>,
        id: u64,
        hint: Option<NodeRef<'gc>>,
    ) -> Option<AnimationPlace<'gc>> {
        let on = |element: NodeRef<'gc>| {
            let data = self.element_animations(element)?;
            let targets = data.targets.borrow();
            let slot = targets.iter().position(|target| {
                target
                    .as_ref()
                    .is_some_and(|target| target.animations.animations().any(|a| a.id == id))
            })?;
            Some(AnimationPlace::Target(element, PSEUDOS[slot]))
        };
        if let Some(found) = hint.and_then(on) {
            return Some(found);
        }
        let data = self.document_data(document)?;
        if let Some(list) = data.animated.get(self.gc)
            && let Some(found) = (0..list.len()).find_map(|index| on(list.get(self.gc, index)?))
        {
            return Some(found);
        }
        data.detached_animations
            .borrow()
            .iter()
            .any(|animation| animation.id == id)
            .then_some(AnimationPlace::Detached)
    }

    /// `f` on `document`'s animation numbered `id` (looked for on `hint`
    /// first) and where it is; `None` if it is gone (script dropped it and
    /// it was let go of). Its element (if any) is styled again with it.
    pub fn with_animation<R>(
        &self,
        document: NodeRef<'gc>,
        id: u64,
        hint: Option<NodeRef<'gc>>,
        f: impl FnOnce(&mut Animation, AnimationPlace<'gc>) -> R,
    ) -> Option<R> {
        let place = self.locate(document, id, hint)?;
        let result = match place {
            AnimationPlace::Target(element, pseudo) => {
                let data = self.element_animations(element)?;
                let mut targets = data.targets.borrow_mut();
                let animation = targets[slot(pseudo)]
                    .as_mut()?
                    .animations
                    .animation_mut(id)?;
                let result = f(animation, place);
                drop(targets);
                data.moved_on.set(true);
                self.mark_style_path(element);
                result
            }
            AnimationPlace::Detached => {
                let data = self.document_data(document)?;
                let mut detached = data.detached_animations.borrow_mut();
                let animation = detached.iter_mut().find(|animation| animation.id == id)?;
                f(animation, place)
            }
        };
        Some(result)
    }

    /// Reads `document`'s animation numbered `id`.
    pub fn read_animation<R>(
        &self,
        document: NodeRef<'gc>,
        id: u64,
        hint: Option<NodeRef<'gc>>,
        f: impl FnOnce(&Animation, AnimationPlace<'gc>) -> R,
    ) -> Option<R> {
        let place = self.locate(document, id, hint)?;
        match place {
            AnimationPlace::Target(element, pseudo) => {
                let data = self.element_animations(element)?;
                let targets = data.targets.borrow();
                let animation = targets[slot(pseudo)]
                    .as_ref()?
                    .animations
                    .animations()
                    .find(|animation| animation.id == id)?;
                Some(f(animation, place))
            }
            AnimationPlace::Detached => {
                let data = self.document_data(document)?;
                let detached = data.detached_animations.borrow();
                let animation = detached.iter().find(|animation| animation.id == id)?;
                Some(f(animation, place))
            }
        }
    }

    /// Takes `document`'s animation numbered `id` off its element's others
    /// or the detached list (style's own lists keep theirs).
    pub fn take_animation(
        &self,
        document: NodeRef<'gc>,
        id: u64,
        hint: Option<NodeRef<'gc>>,
    ) -> Option<Animation> {
        match self.locate(document, id, hint)? {
            AnimationPlace::Target(element, pseudo) => {
                let data = self.element_animations(element)?;
                let mut targets = data.targets.borrow_mut();
                let others = &mut targets[slot(pseudo)].as_mut()?.animations.others;
                let at = others.iter().position(|animation| animation.id == id)?;
                Some(others.remove(at))
            }
            AnimationPlace::Detached => {
                let data = self.document_data(document)?;
                let mut detached = data.detached_animations.borrow_mut();
                let at = detached.iter().position(|animation| animation.id == id)?;
                Some(detached.remove(at))
            }
        }
    }

    /// Puts `animation` on `target`'s others (its animations made, with
    /// `base` as its style without them, if it has none), styled again
    /// with it; off any element without a target.
    pub fn put_animation(
        &self,
        document: NodeRef<'gc>,
        target: Option<(NodeRef<'gc>, Option<PseudoId>)>,
        animation: Animation,
        base: impl FnOnce() -> std::sync::Arc<computed_style::ComputedStyle>,
    ) {
        let Some((element, pseudo)) = target else {
            self.ensure_document_data(document)
                .detached_animations
                .borrow_mut()
                .push(animation);
            return;
        };
        self.style_change_animations(element, pseudo, |slot, _| {
            let entry = slot.get_or_insert_with(|| AnimatedStyle {
                animations: Default::default(),
                base: base(),
            });
            entry.animations.others.push(animation);
            entry
                .animations
                .others
                .sort_by_key(|animation| animation.id);
        });
        if let Some(data) = self.element_animations(element) {
            data.moved_on.set(true);
        }
        self.mark_style_path(element);
    }

    /// Of `document`'s animations, those script can reach without their
    /// objects: those style runs on their elements (`getAnimations()`),
    /// and the others while they play, fill or wait (they may fire
    /// `finish`, or be listed); with their effects' numbers. Their objects
    /// are kept strongly, the others' only weakly (Blink's `Animation`'s
    /// pending activity).
    pub fn prune_animations(&self, document: NodeRef<'gc>) {
        let now = self.animation_time(document);
        let reachable =
            |animation: &Animation| animation.playback.is_pending() || animation.is_reachable(now);
        let mut active = std::collections::HashSet::new();
        if let Some(data) = self.document_data(document) {
            if let Some(list) = data.animated.get(self.gc) {
                for element in (0..list.len()).filter_map(|index| list.get(self.gc, index)) {
                    let Some(animations) = self.element_animations(element) else {
                        continue;
                    };
                    for target in animations.targets.borrow().iter().flatten() {
                        let animations = &target.animations;
                        for animation in animations
                            .transitions
                            .iter()
                            .chain(&animations.css_animations.animations)
                            .chain(animations.others.iter().filter(|a| reachable(a)))
                        {
                            active.insert(animation.id);
                            active.extend(animation.effect_id);
                        }
                    }
                }
            }
            for animation in data
                .detached_animations
                .borrow()
                .iter()
                .filter(|a| reachable(a))
            {
                active.insert(animation.id);
                active.extend(animation.effect_id);
            }
        }
        self.prune_animation_objects(document, |id| active.contains(&id));
        self.prune_effect_objects(document, |id| active.contains(&id));
    }

    fn queue_events_in(
        &self,
        document: NodeRef<'gc>,
        element: NodeRef<'gc>,
        pseudo: Option<PseudoId>,
        sent: Vec<AnimationEvent>,
    ) {
        if sent.is_empty() {
            return;
        }
        let Some(data) = self.document_data(document) else {
            return;
        };
        let id = self.node_id(element);
        data.animation_events
            .borrow_mut()
            .extend(sent.into_iter().map(|event| PendingAnimationEvent {
                element: Some(id),
                pseudo,
                event,
            }));
    }

    /// `sent`, the events of `document`'s animations off any element.
    pub fn queue_detached_animation_events(
        &self,
        document: NodeRef<'gc>,
        sent: Vec<AnimationEvent>,
    ) {
        self.queue_detached_events(document, sent);
    }

    /// The playback notices of `document`'s animation numbered `id` queued
    /// for the next update of animations whose promises aren't settled,
    /// marked settled (their events still wait); the promise notices alone
    /// leave the queue.
    pub fn take_unsettled_playback_notices(
        &self,
        document: NodeRef<'gc>,
        id: u64,
    ) -> Vec<animation::Notice> {
        let Some(data) = self.document_data(document) else {
            return Vec::new();
        };
        let mut notices = Vec::new();
        data.animation_events.borrow_mut().retain_mut(|pending| {
            let AnimationEvent::Playback(playback) = &mut pending.event else {
                return true;
            };
            if playback.id != id || playback.promises_settled {
                return true;
            }
            notices.push(playback.notice);
            playback.promises_settled = true;
            !pending.event.is_promise_notice()
        });
        notices
    }

    /// The playback notices of `document`'s animations queued for the next
    /// update of animations whose promises aren't settled, by animation,
    /// marked settled (their events still wait); the promise notices alone
    /// leave the queue.
    pub fn take_all_unsettled_playback_notices(
        &self,
        document: NodeRef<'gc>,
    ) -> Vec<(u64, animation::Notice)> {
        let Some(data) = self.document_data(document) else {
            return Vec::new();
        };
        let mut notices = Vec::new();
        data.animation_events.borrow_mut().retain_mut(|pending| {
            let AnimationEvent::Playback(playback) = &mut pending.event else {
                return true;
            };
            if playback.promises_settled {
                return true;
            }
            notices.push((playback.id, playback.notice));
            playback.promises_settled = true;
            !pending.event.is_promise_notice()
        });
        notices
    }

    /// `document`'s elements with animations.
    pub fn elements_with_animations(&self, document: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        let Some(list) = self
            .document_data(document)
            .and_then(|data| data.animated.get(self.gc))
        else {
            return Vec::new();
        };
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .collect()
    }

    /// The animation of `document` whose effect is numbered `effect`, and
    /// its target.
    pub fn find_effect(
        &self,
        document: NodeRef<'gc>,
        effect: u64,
    ) -> Option<(u64, Option<EffectTarget<'gc>>)> {
        for element in self.elements_with_animations(document) {
            let found = self.read_animations(element, |targets| {
                targets.iter().enumerate().find_map(|(index, target)| {
                    let animation = target
                        .as_ref()?
                        .animations
                        .animations()
                        .find(|animation| animation.effect_id == Some(effect))?;
                    Some((animation.id, PSEUDOS[index]))
                })
            });
            if let Some(Some((id, pseudo))) = found {
                return Some((id, Some((element, pseudo))));
            }
        }
        let data = self.document_data(document)?;
        let detached = data.detached_animations.borrow();
        let animation = detached
            .iter()
            .find(|animation| animation.effect_id == Some(effect))?;
        Some((animation.id, None))
    }

    /// `sent`, the events of `document`'s animations off any element.
    fn queue_detached_events(&self, document: NodeRef<'gc>, sent: Vec<AnimationEvent>) {
        if sent.is_empty() {
            return;
        }
        let Some(data) = self.document_data(document) else {
            return;
        };
        data.animation_events
            .borrow_mut()
            .extend(sent.into_iter().map(|event| PendingAnimationEvent {
                element: None,
                pseudo: None,
                event,
            }));
    }

    /// Makes `event` a `TransitionEvent` (`transition`) or an
    /// `AnimationEvent` of `name`, `elapsed_time` and `pseudo_element`.
    pub fn set_animation_event_data(
        &self,
        event: Gc<'gc, Event>,
        transition: bool,
        name: &str,
        elapsed_time: f64,
        pseudo_element: &str,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            AnimationEventDataInit {
                transition: Cell::new(transition),
                name: LocalExternal::new(DomString::from(name)),
                elapsed_time: Cell::new(elapsed_time),
                pseudo_element: LocalExternal::new(DomString::from(pseudo_element)),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// `event`'s data, if it is a `TransitionEvent` or an `AnimationEvent`.
    pub fn animation_event_data(&self, event: Gc<'gc, Event>) -> Option<AnimationEventFields<'gc>> {
        let data = event
            .data
            .get(self.gc)?
            .downcast::<AnimationEventData>()?
            .into_ref();
        Some(AnimationEventFields {
            transition: data.transition.get(),
            name: data.name.get(),
            elapsed_time: data.elapsed_time.get(),
            pseudo_element: data.pseudo_element.get(),
        })
    }
}
