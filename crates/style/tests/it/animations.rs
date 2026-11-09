//! CSS transitions and animations in the walk: a style change starts a
//! transition (pending until a frame shows it; the element's, a
//! pseudo-element's), the timeline moving on samples it, a frame applies
//! the values to the base style (a restyle keeps them going, over
//! `!important` too), a CSS animation's keyframes are cascaded on the
//! element (a pseudo-element's too); an inherited property's values reach
//! the descendants (the cascade, not the sample), `em`s follow
//! `font-size`; `display: none` above, leaving the document and a
//! pseudo-element going cancel them; an element removed and back doesn't
//! transition from its style from before; an animated child follows its
//! parent's values its keyframes take. Their events queued in the
//! document.

use std::sync::Arc;

use computed_style::PseudoId;
use dom::{NodeRef, QualName};
use style_resolver::computed_value;

use crate::Page;

impl Page {
    /// The document's timeline moved on to `now` (the frame's update of
    /// animations), and the walk after it; whether they ran.
    fn tick(&mut self, now: f64) -> bool {
        let document = self.document.clone();
        let ran = self
            .dom
            .mutate(|cx| cx.tick_animations(document.get(cx.gc), now));
        self.recalc();
        ran
    }

    /// The pending animations started: a frame showed them.
    fn start_pending(&mut self) {
        let document = self.document.clone();
        self.dom.mutate(|cx| {
            cx.start_pending_animations(document.get(cx.gc));
        });
    }

    /// The events queued since, as `type #id[pseudo]`.
    fn events(&mut self) -> Vec<String> {
        let document = self.document.clone();
        self.dom.mutate(|cx| {
            cx.take_animation_events(document.get(cx.gc))
                .into_iter()
                .filter_map(|pending| {
                    let element = cx.node_by_id(pending.element?)?;
                    let name = pending.event.name()?;
                    let pseudo = match pending.pseudo {
                        None => "",
                        Some(computed_style::PseudoId::Before) => "::before",
                        Some(computed_style::PseudoId::After) => "::after",
                        Some(computed_style::PseudoId::Marker) => "::marker",
                    };
                    Some(format!("{name} #{}{pseudo}", id_of(cx, element)))
                })
                .collect()
        })
    }

    fn has_animations(&mut self) -> bool {
        let document = self.document.clone();
        self.dom.mutate(|cx| cx.has_animations(document.get(cx.gc)))
    }

    fn wants_frames(&mut self) -> bool {
        let document = self.document.clone();
        self.dom
            .mutate(|cx| cx.animations_want_frames(document.get(cx.gc)))
    }

    fn opacity(&mut self, id: &str) -> f32 {
        self.style(id).opacity
    }

    fn before_opacity(&mut self, id: &str) -> Option<f32> {
        Some(self.pseudo(id)?.before.as_ref()?.opacity)
    }
}

fn id_of<'gc>(cx: &dom::Cx<'_, 'gc>, element: NodeRef<'gc>) -> String {
    cx.get_attribute(element, "id")
        .map(|id| id.to_string())
        .unwrap_or_default()
}

/// `#t` fades over 100 ms, from 1 to 0 by the class `out`; `#p` is its
/// grandparent.
const TRANSITION: &str = "<!doctype html><style>#t { opacity: 1; transition: opacity 100ms linear }\
    #t.out { opacity: 0 } .none { display: none }</style><div id=p><div><div id=t></div></div></div>";

fn fade(page: &mut Page) {
    page.with("t", |cx, t| cx.set_attribute(t, "class", "out"));
    page.recalc();
}

#[test]
fn a_style_change_starts_a_transition_which_the_timeline_moves_on() {
    let mut page = Page::new(TRANSITION);
    assert!(!page.has_animations(), "none at first");
    fade(&mut page);
    // Pending: its start value until a frame shows it; its events at the
    // first frame.
    assert_eq!(page.opacity("t"), 1.0);
    assert!(page.events().is_empty());
    assert!(page.has_animations());
    page.start_pending();
    assert!(page.wants_frames());
    assert!(page.tick(25.0));
    assert_eq!(page.opacity("t"), 0.75);
    assert_eq!(page.events(), ["transitionrun #t", "transitionstart #t"]);
    assert!(page.tick(50.0));
    assert_eq!(page.opacity("t"), 0.5, "sampled again");
    assert!(page.tick(100.0));
    assert_eq!(page.opacity("t"), 0.0);
    assert_eq!(page.events(), ["transitionend #t"]);
    // Done: nothing runs, and at the next frame it goes (its end value
    // kept: the same change doesn't start it again).
    assert!(!page.wants_frames());
    assert!(!page.tick(150.0));
    assert_eq!(page.opacity("t"), 0.0);
    let left = page.with("t", |cx, t| {
        cx.read_animations(t, |targets| {
            targets[0]
                .as_ref()
                .map(|target| target.animations.animations().count())
        })
    });
    assert_eq!(left, Some(Some(0)));
}

#[test]
fn display_none_above_cancels_them() {
    let mut page = Page::new(TRANSITION);
    fade(&mut page);
    page.start_pending();
    page.tick(25.0);
    page.events();
    // Its grandparent's: the whole subtree is styled again.
    page.with("p", |cx, p| cx.set_attribute(p, "class", "none"));
    page.recalc();
    assert_eq!(page.events(), ["transitioncancel #t"]);
    assert_eq!(page.opacity("t"), 0.0, "its value without it");
    assert!(!page.wants_frames());
}

#[test]
fn display_none_on_it_cancels_them() {
    let mut page = Page::new(TRANSITION);
    fade(&mut page);
    page.start_pending();
    page.tick(25.0);
    page.events();
    page.with("t", |cx, t| cx.set_attribute(t, "class", "out none"));
    page.recalc();
    assert_eq!(page.events(), ["transitioncancel #t"]);
    assert!(!page.wants_frames());
}

#[test]
fn leaving_the_document_cancels_them_at_the_next_frame() {
    let mut page = Page::new(TRANSITION);
    fade(&mut page);
    page.start_pending();
    page.tick(25.0);
    page.events();
    page.with("t", |cx, t| cx.remove(t));
    assert!(!page.tick(50.0));
    assert_eq!(page.events(), ["transitioncancel #t"]);
    assert!(!page.has_animations());
}

#[test]
fn an_element_removed_and_back_doesnt_transition_from_before() {
    let mut page = Page::new(TRANSITION);
    // Animations seen: removals are recorded from then on.
    fade(&mut page);
    page.start_pending();
    page.tick(100.0);
    page.tick(200.0);
    page.events();
    // Back to 1 while out of the document, then in again: no transition.
    page.with("t", |cx, t| {
        let parent = cx.parent(t).unwrap();
        cx.remove(t);
        cx.set_attribute(t, "class", "");
        cx.append_child(parent, t).unwrap();
    });
    page.recalc();
    assert_eq!(page.opacity("t"), 1.0);
    assert!(page.events().is_empty());
    assert!(!page.wants_frames());
    // Styled in its tree since: a change transitions again.
    fade(&mut page);
    assert!(page.has_animations());
    page.start_pending();
    page.tick(300.0);
    page.tick(400.0);
    page.events();
    // Below a subtree removed and back: no transition either.
    page.with("p", |cx, p| {
        let parent = cx.parent(p).unwrap();
        cx.remove(p);
        let t = cx.first_child(cx.first_child(p).unwrap()).unwrap();
        cx.set_attribute(t, "class", "");
        cx.append_child(parent, p).unwrap();
    });
    page.recalc();
    assert_eq!(page.opacity("t"), 1.0);
    assert!(!page.wants_frames());
}

#[test]
fn a_css_animations_keyframes_are_cascaded_on_it() {
    let mut page = Page::new(
        "<!doctype html><style>@keyframes fade { from { opacity: 0 } to { opacity: var(--end) } }\
         #a { --end: 0.5; animation: fade 100ms linear } #a.b::before { content: ''; animation: fade 100ms linear }\
         </style><div id=a class=b></div>",
    );
    assert_eq!(page.opacity("a"), 0.0, "its first keyframe, pending");
    assert_eq!(page.events(), Vec::<String>::new());
    page.start_pending();
    page.tick(50.0);
    assert_eq!(page.opacity("a"), 0.25, "its own variable's end");
    assert_eq!(
        page.before_opacity("a"),
        Some(0.25),
        "the pseudo-element inherits it"
    );
    assert_eq!(
        page.events(),
        ["animationstart #a", "animationstart #a::before"]
    );
    // The `::before` with no box, then gone: its animation is canceled.
    page.with("a", |cx, a| {
        let document = cx.document_of(a);
        let style = cx.create_element(document, QualName::html("style"));
        let text = cx.create_text(document, "#a.b::before { content: none }");
        cx.append_child(style, text).unwrap();
        cx.append_child(a, style).unwrap();
    });
    page.recalc();
    assert_eq!(page.events(), ["animationcancel #a::before"]);
    page.with("a", |cx, a| {
        let style = cx.first_child(a).unwrap();
        cx.remove(style);
    });
    page.recalc();
    page.start_pending();
    page.tick(60.0);
    page.events();
    page.with("a", |cx, a| cx.set_attribute(a, "class", ""));
    page.recalc();
    assert_eq!(
        page.events(),
        ["animationcancel #a::before"],
        "no rule for it"
    );
    page.tick(100.0);
    assert_eq!(page.events(), ["animationend #a"]);
}

#[test]
fn an_animated_child_follows_the_values_its_keyframes_take() {
    let mut page = Page::new(
        "<!doctype html><style>@keyframes take { from { opacity: inherit } to { opacity: inherit } }\
         #p { opacity: 0.5 } #p.more { opacity: 0.75 } #c { animation: take 100ms linear }</style>\
         <div id=p><div id=c></div></div>",
    );
    page.start_pending();
    page.tick(10.0);
    assert_eq!(page.opacity("c"), 0.5);
    page.with("p", |cx, p| cx.set_attribute(p, "class", "more"));
    page.recalc();
    assert_eq!(
        page.opacity("c"),
        0.75,
        "its parent's new value, though not inherited"
    );
}

/// What a frame's lifecycle does, for the tests that read values as
/// `getComputedStyle` writes them: the page styled with what it starts
/// started at once, frames at a time.
impl Page {
    /// `html`, parsed and styled at time 0, what it starts started.
    pub(crate) fn timed(html: &str) -> Self {
        let mut page = Self::new(html);
        page.start_pending();
        page
    }

    /// Styles the page as a frame's lifecycle does: the animations it
    /// starts start at the timeline's time.
    pub(crate) fn restyle(&mut self) {
        self.recalc();
        self.start_pending();
    }

    /// Sets `#{id}`'s class and styles the page again.
    pub(crate) fn set_class(&mut self, id: &str, class: &str) {
        self.with(id, |cx, element| cx.set_attribute(element, "class", class));
        self.restyle();
    }

    /// The timeline at `ms`, as a frame would set it, without styling.
    pub(crate) fn set_time(&mut self, ms: f64) {
        self.with_document(|cx, document| {
            if cx.timeline_time(document).is_none() {
                cx.start_timeline(document, ms);
            } else {
                cx.tick_animations(document, ms);
            }
        });
    }

    /// A frame at `ms`: the animations ticked (their events queued, the
    /// elements with ones that ran marked), and the page styled.
    pub(crate) fn frame(&mut self, ms: f64) {
        self.with_document(|cx, document| {
            cx.start_timeline(document, ms);
            cx.tick_animations(document, ms);
        });
        self.restyle();
    }

    pub(crate) fn value(&mut self, id: &str, property: &str) -> String {
        computed_value(&self.style(id), property).unwrap()
    }

    pub(crate) fn pseudo_value(&mut self, id: &str, pseudo: PseudoId, property: &str) -> String {
        let pseudos = self.pseudo(id).expect("pseudo-elements");
        computed_value(pseudos.get(pseudo).expect("the pseudo-element"), property).unwrap()
    }

    /// `#{id}`'s (or its `pseudo`'s) animations, read by `read`; `None`
    /// if it has none.
    pub(crate) fn read_target<R>(
        &mut self,
        id: &str,
        pseudo: Option<PseudoId>,
        read: impl FnOnce(&animation::AnimatedStyle) -> R,
    ) -> Option<R> {
        let slot = match pseudo {
            None => 0,
            Some(PseudoId::Before) => 1,
            Some(PseudoId::After) => 2,
            Some(PseudoId::Marker) => 3,
        };
        self.with(id, |cx, element| {
            cx.read_animations(element, |targets| targets[slot].as_ref().map(read))
                .flatten()
        })
    }

    /// Whether style runs animations on `#{id}`: CSS animations or
    /// transitions (not those it canceled).
    pub(crate) fn has_style_animations(&mut self, id: &str) -> bool {
        self.read_target(id, None, |target| {
            !target.animations.css_animations.is_empty()
                || !target.animations.transitions.is_empty()
        })
        .unwrap_or(false)
    }

    /// Whether `#{id}` (or its `pseudo`) has animations kept on it.
    pub(crate) fn animations_on(&mut self, id: &str, pseudo: Option<PseudoId>) -> bool {
        self.read_target(id, pseudo, |_| ()).is_some()
    }
}

const WIDTH: &str = "<style>
  #t { width: 0px; transition: width 1s linear }
  #t.wide { width: 100px }
  #t.none { display: none }
</style><div id=t><span id=child></span></div>";

#[test]
fn a_style_change_starts_a_transition() {
    let mut page = Page::timed(WIDTH);
    assert!(
        !page.animations_on("t", None),
        "a first style starts nothing"
    );
    page.set_time(1000.0);
    page.set_class("t", "wide");
    assert_eq!(
        page.value("t", "width"),
        "0px",
        "the start value at its start"
    );
    assert!(page.animations_on("t", None));
    page.frame(1250.0);
    assert_eq!(page.value("t", "width"), "25px");
    page.frame(1500.0);
    assert_eq!(page.value("t", "width"), "50px");
    page.frame(2000.0);
    assert_eq!(page.value("t", "width"), "100px");
}

/// A frame only applies the values to the base style: the subtree isn't
/// styled again for a property it doesn't inherit.
#[test]
fn a_frame_leaves_the_subtree() {
    let mut page = Page::timed(WIDTH);
    page.set_class("t", "wide");
    let child = page.style("child");
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "50px");
    assert!(
        Arc::ptr_eq(&child, &page.style("child")),
        "not styled again"
    );
}

/// Something else changing styles the element again: its base style
/// is the new one, the transition goes on.
#[test]
fn a_restyle_keeps_the_transition() {
    let mut page = Page::timed(
        "<style>#t { width: 0px; transition: width 1s linear } #t.wide { width: 100px }
         #t.tall { height: 9px }</style><div id=t></div>",
    );
    page.set_class("t", "wide");
    page.frame(500.0);
    page.set_class("t", "wide tall");
    assert_eq!(page.value("t", "width"), "50px");
    assert_eq!(page.value("t", "height"), "9px");
    // The next frame applies them to the new base style.
    page.frame(750.0);
    assert_eq!(page.value("t", "width"), "75px");
    assert_eq!(page.value("t", "height"), "9px");
}

/// A delay alone is time enough: a duration of 0 after it.
#[test]
fn a_delay_alone_starts_one() {
    let mut page = Page::timed(
        "<style>#t { width: 0px; transition: width 0s 1s } #t.wide { width: 100px }</style>\
         <div id=t></div>",
    );
    page.set_class("t", "wide");
    assert!(page.animations_on("t", None));
    page.frame(500.0);
    assert_eq!(
        page.value("t", "width"),
        "0px",
        "held until its delay is over"
    );
}

#[test]
fn display_none_cancels_and_starts_nothing() {
    let mut page = Page::timed(WIDTH);
    page.set_class("t", "wide");
    page.frame(500.0);
    page.set_class("t", "none");
    assert!(!page.animations_on("t", None), "canceled");
    // Shown again: no transition from a style it didn't have.
    page.set_class("t", "wide");
    assert!(!page.animations_on("t", None));
    assert_eq!(page.value("t", "width"), "100px");
    // Under an element with `display: none` it isn't rendered either.
    drop(page);
    let mut page = Page::timed(
        "<style>#t { width: 0px; transition: width 1s linear } #t.wide { width: 100px }
         #p.none { display: none }</style><div id=p><div id=t></div></div>",
    );
    page.set_class("t", "wide");
    assert!(page.animations_on("t", None));
    page.set_class("p", "none");
    assert!(!page.animations_on("t", None), "canceled under it");
}

/// An inherited property's value goes down to the descendants, which are
/// styled again at each frame.
#[test]
fn descendants_inherit_the_values() {
    let mut page = Page::timed(
        "<style>#t { color: rgb(0, 0, 0); transition: color 1s linear }
         #t.red { color: rgb(200, 0, 0) }
         #t::before { content: 'x' }</style><div id=t><span id=child></span></div>",
    );
    page.set_class("t", "red");
    page.frame(500.0);
    assert_eq!(page.value("t", "color"), "rgb(100, 0, 0)");
    assert_eq!(page.value("child", "color"), "rgb(100, 0, 0)");
    assert_eq!(
        page.pseudo_value("t", PseudoId::Before, "color"),
        "rgb(100, 0, 0)",
        "its pseudo-elements too"
    );
    page.frame(750.0);
    assert_eq!(page.value("child", "color"), "rgb(150, 0, 0)");
}

/// A transition of `font-size`: the element's `em`s follow it.
#[test]
fn ems_follow_font_size() {
    let mut page = Page::timed(
        "<style>#t { font-size: 10px; padding-left: 2em; transition: font-size 1s linear }
         #t.big { font-size: 20px }</style><div id=t></div>",
    );
    page.set_class("t", "big");
    page.frame(500.0);
    assert_eq!(page.value("t", "font-size"), "15px");
    assert_eq!(page.value("t", "padding-left"), "30px");
    page.frame(2000.0);
    assert_eq!(page.value("t", "padding-left"), "40px");
}

/// A pseudo-element's own transitions; gone, they are forgotten.
#[test]
fn pseudo_elements_transition() {
    let mut page = Page::timed(
        "<style>#t::before { content: 'x'; width: 0px; transition: width 1s linear }
         #t.wide::before { width: 100px }
         #t.gone::before { content: none }</style><div id=t></div>",
    );
    page.set_class("t", "wide");
    page.frame(250.0);
    assert_eq!(page.pseudo_value("t", PseudoId::Before, "width"), "25px");
    assert!(page.animations_on("t", Some(PseudoId::Before)));
    assert!(!page.animations_on("t", None), "the element has none");
    page.set_class("t", "gone");
    assert!(!page.animations_on("t", Some(PseudoId::Before)));
    // No rule for it at all any more.
    drop(page);
    let mut page = Page::timed(
        "<style>#t.on::before { content: 'x'; width: 0px; transition: width 1s linear }
         #t.on.wide::before { width: 100px }</style><div id=t class=on></div>",
    );
    page.set_class("t", "on wide");
    assert!(page.animations_on("t", Some(PseudoId::Before)));
    page.set_class("t", "");
    assert!(!page.animations_on("t", Some(PseudoId::Before)));
}

/// A transition beats `!important` (css-cascade 4: the transition origin
/// is the highest).
#[test]
fn transitions_beat_important() {
    let mut page = Page::timed(
        "<style>#t { width: 0px !important; transition: width 1s linear }
         #t.wide { width: 100px !important }</style><div id=t></div>",
    );
    page.set_class("t", "wide");
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "50px");
}
