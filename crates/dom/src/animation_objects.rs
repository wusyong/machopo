//! The script objects of animations (web-animations 1's `Animation`,
//! `CSSTransition`, `CSSAnimation`, `KeyframeEffect`, `DocumentTimeline`):
//! an animation's object is an event target (its `finish` and `cancel`
//! events go to it) whose data holds the animation's number, its class,
//! its document, where it last was (its target element: where to look
//! first) and, once asked for, its `ready` and `finished` promises'
//! resolvers (`JsEdge`s the engine writes, Blink's `ScriptPromiseProperty`
//! on its `Animation`). The animation itself is a value on its element
//! (`animations`) or, without one, in its document's detached list.
//!
//! A document keeps its animations' objects weakly, to give script the
//! same one each time (Blink's `Animation` is the object), and strongly
//! while the animation can still be reached (`prune_animation_objects`:
//! on its element, where `getAnimations()` finds it, or running after it
//! left; Blink's element keeps its `Animation`, and one with listeners
//! has pending activity). An effect's object likewise. Its `timeline` is
//! the same object each time, which the document keeps.

use std::cell::{Cell, RefCell};

use animation::Animation;
use elise::{Gc, GcObject, HeapVector, JsEdge, LocalExternal, Member, WeakMember};

use crate::{Cx, DomString, Event, EventTargetImpl, Node, NodeRef, sites};

/// Which interface an animation's object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationClass {
    /// `Animation`: script made it.
    Script,
    CssTransition,
    CssAnimation,
}

impl AnimationClass {
    fn bits(self) -> u8 {
        match self {
            Self::Script => 0,
            Self::CssTransition => 1,
            Self::CssAnimation => 2,
        }
    }

    fn of_bits(bits: u8) -> Self {
        match bits {
            1 => Self::CssTransition,
            2 => Self::CssAnimation,
            _ => Self::Script,
        }
    }

    /// The class of `animation`, by its owner.
    pub fn of(animation: &Animation) -> Self {
        match animation.owner {
            animation::Owner::Transition(_) => Self::CssTransition,
            animation::Owner::CssAnimation(_) => Self::CssAnimation,
            animation::Owner::Script => Self::Script,
        }
    }
}

/// An animation's object's data.
#[derive(GcObject)]
pub struct AnimationObjectData {
    id: Cell<u64>,
    class: Cell<u8>,
    /// Set at creation, never changes.
    document: Member<Node>,
    /// The element its animation was last found on (none: detached), which
    /// a member looks at first.
    target: Member<Node>,
    /// Its `id` (script's name of it).
    name: LocalExternal<RefCell<DomString>>,
    /// Its `ready` and `finished` promises' resolvers, once asked for: the
    /// engine's (empty: none, or settled and replaced).
    ready: JsEdge,
    finished: JsEdge,
    /// What its animation's playback did to its promises since the engine
    /// last settled them, in order (`take_animation_notices`).
    notices: LocalExternal<RefCell<Vec<animation::Notice>>>,
}

/// An object held weakly, on its document's list.
#[derive(GcObject)]
pub struct AnimationObjectRef {
    object: WeakMember<EventTargetImpl>,
}

/// An effect's object (Blink's `KeyframeEffect`): the effect's number and
/// its document. The effect itself is in its animation, or, without one
/// (`new KeyframeEffect()`'s not given to one, one taken off its
/// animation), here: only this object reaches it.
#[derive(GcObject)]
pub struct KeyframeEffectObject {
    id: Cell<u64>,
    /// Set at creation, never changes.
    document: Member<Node>,
    /// Its effect while no animation has it. A `RefCell` as the other
    /// fields' cells: the object is reached shared (`Gc`); borrowed only
    /// inside the `Cx` functions below, never across a call out.
    free: LocalExternal<RefCell<Option<FreeEffect>>>,
    /// A free effect's target's element (its pseudo-element in its timing
    /// input).
    free_target: Member<Node>,
    wrapper: JsEdge,
}

/// An effect without an animation: its keyframes, its timing and how
/// script wrote it.
#[derive(Debug, Clone)]
pub struct FreeEffect {
    pub effect: std::sync::Arc<animation::KeyframeEffect>,
    pub timing: animation::Timing,
    pub input: animation::TimingInput,
}

#[derive(GcObject)]
pub struct KeyframeEffectRef {
    object: WeakMember<KeyframeEffectObject>,
}

/// A document's `timeline` (Blink's `DocumentTimeline`): its default
/// timeline, whose time is its frame's animation time.
#[derive(GcObject)]
pub struct DocumentTimeline {
    /// Set at creation, never changes.
    document: Member<Node>,
    wrapper: JsEdge,
}

/// An `AnimationPlaybackEvent`'s data: the animation's current time and
/// its timeline's time when it happened.
#[derive(GcObject)]
pub struct AnimationPlaybackEventData {
    current_time: Cell<Option<f64>>,
    timeline_time: Cell<Option<f64>>,
}

impl<'gc> Cx<'_, 'gc> {
    /// Makes `event` an `AnimationPlaybackEvent` of `current_time` and
    /// `timeline_time`.
    pub fn set_animation_playback_event_data(
        &self,
        event: Gc<'gc, Event>,
        current_time: Option<f64>,
        timeline_time: Option<f64>,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            AnimationPlaybackEventDataInit {
                current_time: Cell::new(current_time),
                timeline_time: Cell::new(timeline_time),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// `event`'s `currentTime` and `timelineTime`, if it is an
    /// `AnimationPlaybackEvent`.
    pub fn animation_playback_event_data(
        &self,
        event: Gc<'gc, Event>,
    ) -> Option<(Option<f64>, Option<f64>)> {
        let data = event
            .data
            .get(self.gc)?
            .downcast::<AnimationPlaybackEventData>()?;
        Some((data.current_time.get(), data.timeline_time.get()))
    }

    fn animation_data(
        &self,
        object: Gc<'gc, EventTargetImpl>,
    ) -> Option<Gc<'gc, AnimationObjectData>> {
        object.animation.get(self.gc)
    }

    /// Whether `target` is an animation's object.
    pub fn is_animation_object(&self, target: Gc<'gc, EventTargetImpl>) -> bool {
        self.animation_data(target).is_some()
    }

    /// The animation `object` stands for: its number, class and document.
    pub fn animation_of_object(
        &self,
        object: Gc<'gc, EventTargetImpl>,
    ) -> Option<(u64, AnimationClass, NodeRef<'gc>)> {
        let data = self.animation_data(object)?;
        let document = data
            .document
            .get(self.gc)
            .expect("an animation's object has its document");
        Some((
            data.id.get(),
            AnimationClass::of_bits(data.class.get()),
            document,
        ))
    }

    /// The element `object`'s animation was last found on.
    pub fn animation_object_target(
        &self,
        object: Gc<'gc, EventTargetImpl>,
    ) -> Option<NodeRef<'gc>> {
        self.animation_data(object)?.target.get(self.gc)
    }

    /// `object`'s animation was found on `target` (none: detached).
    pub fn set_animation_object_target(
        &self,
        object: Gc<'gc, EventTargetImpl>,
        target: Option<NodeRef<'gc>>,
    ) {
        if let Some(data) = self.animation_data(object) {
            data.target.set(self.gc, target);
        }
    }

    /// `object`'s `id`.
    pub fn animation_object_name(&self, object: Gc<'gc, EventTargetImpl>) -> DomString {
        self.animation_data(object)
            .map_or_else(|| DomString::new(""), |data| data.name.borrow().clone())
    }

    pub fn set_animation_object_name(&self, object: Gc<'gc, EventTargetImpl>, name: &str) {
        if let Some(data) = self.animation_data(object) {
            *data.name.borrow_mut() = DomString::from(name);
        }
    }

    /// `object`'s `ready` (`ready`) or `finished` promise's resolver's
    /// edge, which the engine reads and writes.
    pub fn animation_promise_edge(
        &self,
        object: Gc<'gc, EventTargetImpl>,
        ready: bool,
    ) -> Option<&'gc JsEdge> {
        let data = self.animation_data(object)?.into_ref();
        Some(if ready { &data.ready } else { &data.finished })
    }

    /// `notice`, what `object`'s animation's playback did to its promises,
    /// queued for the engine, which settles them in order before the call
    /// returns (`Microtasks::ANIMATIONS`) or before it gives one.
    pub fn push_animation_notice(
        &self,
        object: Gc<'gc, EventTargetImpl>,
        notice: animation::Notice,
    ) {
        let Some(data) = self.animation_data(object) else {
            return;
        };
        let mut notices = data.notices.borrow_mut();
        let first = notices.is_empty();
        notices.push(notice);
        if !first {
            return;
        }
        let state = &self.changes.animations;
        let list = match state.noticed.take() {
            Some(list) => list,
            None => elise::Persistent::new(self.gc, HeapVector::new(self.gc, 1)),
        };
        list.get(self.gc).push(self.gc, object);
        state.noticed.set(Some(list));
        self.changes
            .microtasks
            .set(self.changes.microtasks.get() | crate::Microtasks::ANIMATIONS);
    }

    /// The notices queued for `object`'s promises, taken.
    pub fn take_animation_notices(
        &self,
        object: Gc<'gc, EventTargetImpl>,
    ) -> Vec<animation::Notice> {
        self.animation_data(object)
            .map(|data| std::mem::take(&mut *data.notices.borrow_mut()))
            .unwrap_or_default()
    }

    /// The objects notices were queued for since, taken.
    pub fn take_objects_with_animation_notices(&self) -> Vec<Gc<'gc, EventTargetImpl>> {
        let Some(list) = self.changes.animations.noticed.take() else {
            return Vec::new();
        };
        let list = list.get(self.gc);
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .collect()
    }

    /// The object of `document`'s animation numbered `id`, if script has
    /// one alive.
    pub fn existing_animation_object(
        &self,
        document: NodeRef<'gc>,
        id: u64,
    ) -> Option<Gc<'gc, EventTargetImpl>> {
        let list = self
            .document_data(document)?
            .animation_objects
            .get(self.gc)?;
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index)?.object.get(self.gc))
            .find(|&object| {
                self.animation_data(object)
                    .is_some_and(|data| data.id.get() == id)
            })
    }

    /// The object of `document`'s animation numbered `id` (of `class`,
    /// found on `target`): the one script has, or a new one, kept while
    /// its animation can be reached.
    pub fn animation_object(
        &self,
        document: NodeRef<'gc>,
        id: u64,
        class: AnimationClass,
        target: Option<NodeRef<'gc>>,
    ) -> Gc<'gc, EventTargetImpl> {
        if let Some(object) = self.existing_animation_object(document, id) {
            return object;
        }
        let object = self.create_event_target();
        let data = self.gc.alloc_in(
            &sites::OTHER,
            AnimationObjectDataInit {
                id: Cell::new(id),
                class: Cell::new(class.bits()),
                document: Some(document),
                target,
                name: LocalExternal::new(RefCell::new(DomString::new(""))),
                ready: (),
                finished: (),
                notices: LocalExternal::new(RefCell::new(Vec::new())),
            },
        );
        object.animation.set(self.gc, Some(data));
        let data = self.ensure_document_data(document).into_ref();
        let entry = self.gc.alloc_in(
            &sites::OTHER,
            AnimationObjectRefInit {
                object: Some(object),
            },
        );
        push(self, &data.animation_objects, entry);
        crate::intersection::prune(
            self,
            data.animation_objects.get(self.gc).expect("pushed"),
            |entry| entry.object.get(self.gc).is_some(),
        );
        push(self, &data.kept_animation_objects, object);
        object
    }

    /// Of `document`'s animations' objects, keeps strongly those `kept`
    /// says can still be reached (by their numbers), the others only
    /// weakly; the numbers of those alive (`kept` or not).
    pub fn prune_animation_objects(
        &self,
        document: NodeRef<'gc>,
        kept: impl Fn(u64) -> bool,
    ) -> Vec<u64> {
        let Some(data) = self.document_data(document) else {
            return Vec::new();
        };
        let data = data.into_ref();
        if let Some(list) = data.kept_animation_objects.get(self.gc) {
            for index in (0..list.len()).rev() {
                let keep = list
                    .get(self.gc, index)
                    .and_then(|object| self.animation_data(object))
                    .is_some_and(|data| kept(data.id.get()));
                if !keep {
                    list.remove(self.gc, index);
                }
            }
        }
        let Some(list) = data.animation_objects.get(self.gc) else {
            return Vec::new();
        };
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index)?.object.get(self.gc))
            .filter_map(|object| Some(self.animation_data(object)?.id.get()))
            .collect()
    }

    /// Keeps `object` strongly again (its animation can be reached again:
    /// script played it), if it isn't.
    pub fn keep_animation_object(&self, document: NodeRef<'gc>, object: Gc<'gc, EventTargetImpl>) {
        let data = self.ensure_document_data(document).into_ref();
        if let Some(list) = data.kept_animation_objects.get(self.gc)
            && (0..list.len()).any(|index| {
                list.get(self.gc, index)
                    .is_some_and(|kept| Gc::ptr_eq(kept, object))
            })
        {
            return;
        }
        push(self, &data.kept_animation_objects, object);
    }

    /// The object of `document`'s effect numbered `id`: the one script has,
    /// or a new one, kept while its animation can be reached.
    pub fn effect_object(&self, document: NodeRef<'gc>, id: u64) -> Gc<'gc, KeyframeEffectObject> {
        let data = self.ensure_document_data(document).into_ref();
        if let Some(list) = data.effect_objects.get(self.gc)
            && let Some(object) = (0..list.len())
                .filter_map(|index| list.get(self.gc, index)?.object.get(self.gc))
                .find(|object| object.id.get() == id)
        {
            return object;
        }
        let object = self.gc.alloc_in(
            &sites::OTHER,
            KeyframeEffectObjectInit {
                id: Cell::new(id),
                document: Some(document),
                free: LocalExternal::new(RefCell::new(None)),
                free_target: None,
                wrapper: (),
            },
        );
        let entry = self.gc.alloc_in(
            &sites::OTHER,
            KeyframeEffectRefInit {
                object: Some(object),
            },
        );
        push(self, &data.effect_objects, entry);
        crate::intersection::prune(
            self,
            data.effect_objects.get(self.gc).expect("pushed"),
            |entry| entry.object.get(self.gc).is_some(),
        );
        push(self, &data.kept_effect_objects, object);
        object
    }

    /// Of `document`'s effects' objects, keeps strongly those whose
    /// numbers `kept` says (their animations can be reached).
    pub fn prune_effect_objects(&self, document: NodeRef<'gc>, kept: impl Fn(u64) -> bool) {
        let Some(list) = self
            .document_data(document)
            .and_then(|data| data.kept_effect_objects.get(self.gc))
        else {
            return;
        };
        for index in (0..list.len()).rev() {
            if !list
                .get(self.gc, index)
                .is_some_and(|object| kept(object.id.get()))
            {
                list.remove(self.gc, index);
            }
        }
    }

    /// The effect `object` stands for: its number and document.
    pub fn effect_of_object(&self, object: Gc<'gc, KeyframeEffectObject>) -> (u64, NodeRef<'gc>) {
        let object = object.into_ref();
        (
            object.id.get(),
            object
                .document
                .get(self.gc)
                .expect("an effect's object has its document"),
        )
    }

    /// `object`'s effect if it has no animation, and its target's element.
    pub fn free_effect(
        &self,
        object: Gc<'gc, KeyframeEffectObject>,
    ) -> Option<(FreeEffect, Option<NodeRef<'gc>>)> {
        let free = object.free.borrow().clone()?;
        Some((free, object.free_target.get(self.gc)))
    }

    /// Changes `object`'s effect without an animation, if it is one.
    pub fn with_free_effect<R>(
        &self,
        object: Gc<'gc, KeyframeEffectObject>,
        f: impl FnOnce(&mut FreeEffect) -> R,
    ) -> Option<R> {
        Some(f(object.free.borrow_mut().as_mut()?))
    }

    /// `object`'s effect without an animation taken (for one), with its
    /// target's element.
    pub fn take_free_effect(
        &self,
        object: Gc<'gc, KeyframeEffectObject>,
    ) -> Option<(FreeEffect, Option<NodeRef<'gc>>)> {
        let free = object.free.borrow_mut().take()?;
        let target = object.free_target.get(self.gc);
        object.free_target.set(self.gc, None);
        Some((free, target))
    }

    /// `free`, an effect without an animation now, kept by its `object`,
    /// its target `target`'s element.
    pub fn put_free_effect(
        &self,
        object: Gc<'gc, KeyframeEffectObject>,
        free: FreeEffect,
        target: Option<NodeRef<'gc>>,
    ) {
        *object.free.borrow_mut() = Some(free);
        object.free_target.set(self.gc, target);
    }

    /// Points `object`'s effect without an animation at `target`'s element.
    pub fn set_free_effect_target(
        &self,
        object: Gc<'gc, KeyframeEffectObject>,
        target: Option<NodeRef<'gc>>,
    ) {
        object.free_target.set(self.gc, target);
    }

    pub(crate) fn effect_wrapper_edge(&self, object: Gc<'gc, KeyframeEffectObject>) -> &'gc JsEdge {
        &object.into_ref().wrapper
    }

    /// `document.timeline`, the same each time.
    pub fn document_timeline(&self, document: NodeRef<'gc>) -> Gc<'gc, DocumentTimeline> {
        let data = self.ensure_document_data(document).into_ref();
        if let Some(timeline) = data.timeline.get(self.gc) {
            return timeline;
        }
        let timeline = self.gc.alloc_in(
            &sites::OTHER,
            DocumentTimelineInit {
                document: Some(document),
                wrapper: (),
            },
        );
        data.timeline.set(self.gc, Some(timeline));
        timeline
    }

    /// The document whose `timeline` `timeline` is.
    pub fn timeline_document(&self, timeline: Gc<'gc, DocumentTimeline>) -> NodeRef<'gc> {
        timeline
            .document
            .get(self.gc)
            .expect("a timeline has its document")
    }

    pub(crate) fn timeline_wrapper_edge(&self, timeline: Gc<'gc, DocumentTimeline>) -> &'gc JsEdge {
        &timeline.into_ref().wrapper
    }
}

/// `value` last on the list at `edge`, made first if there is none.
fn push<'gc, T: GcObject>(cx: &Cx<'_, 'gc>, edge: &Member<HeapVector<T>>, value: Gc<'gc, T>) {
    let list = match edge.get(cx.gc) {
        Some(list) => list,
        None => {
            let list = HeapVector::new(cx.gc, 1);
            edge.set(cx.gc, Some(list));
            list
        }
    };
    list.push(cx.gc, value);
}
