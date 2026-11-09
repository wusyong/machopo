//! CSS animations in style recalc: an element's `animation-name` starts,
//! keeps, updates and cancels its animations; their keyframes are
//! computed on it (`var()`, `em`, the missing 0% and 100% its own values),
//! cascaded at the animation origin (under `!important`, under
//! transitions); a frame gives the values between the keyframes around
//! the progress, as the timing (delay, fill, iterations, direction, play
//! state) and each keyframe's easing say.

use std::sync::Arc;

use animation::{AnimationEvent, KeyframeEffect};

use crate::{Page, attach_shadow};

/// `#t`'s animations' keyframe effects, in order.
fn effects(page: &mut Page) -> Vec<Arc<KeyframeEffect>> {
    page.read_target("t", None, |target| {
        target
            .animations
            .css_animations
            .animations
            .iter()
            .map(|a| a.effect.clone())
            .collect()
    })
    .unwrap_or_default()
}

/// The CSS animation events queued since the last call, in dispatch
/// order: each as `kind name elapsed@scheduled`.
fn events(page: &mut Page) -> Vec<String> {
    let mut queued = page.with_document(|cx, document| cx.take_animation_events(document));
    queued.sort_by(|a, b| a.event.dispatch_order(&b.event));
    queued
        .into_iter()
        .filter_map(|pending| match pending.event {
            AnimationEvent::Animation(e) => Some(format!(
                "{} {} {}@{}",
                e.kind.name(),
                e.name,
                e.elapsed_time,
                e.scheduled_time
            )),
            AnimationEvent::Transition(_) | AnimationEvent::Playback(_) => None,
        })
        .collect()
}

const GROW: &str = "<style>
  @keyframes grow { from { width: 0px } to { width: 100px } }
  @keyframes half { 50% { width: 100px } }
  #t { width: 50px; animation: grow 1s linear }
  #t.half { animation: half 1s linear }
  #t.none { animation: none }
  #t.hidden { display: none }
</style><div id=t></div>";

#[test]
fn an_animation_starts_with_its_element() {
    let mut page = Page::timed(GROW);
    assert!(page.animations_on("t", None));
    assert_eq!(page.value("t", "width"), "0px", "its first keyframe at 0");
    page.frame(250.0);
    assert_eq!(page.value("t", "width"), "25px");
    page.frame(750.0);
    assert_eq!(page.value("t", "width"), "75px");
    // Done, with no fill: the element's own value.
    page.frame(1000.0);
    assert_eq!(page.value("t", "width"), "50px");
    page.frame(1100.0);
    assert_eq!(page.value("t", "width"), "50px");
    let running = page
        .read_target("t", None, |target| target.animations.is_running())
        .expect("kept once done");
    assert!(!running, "done: no more frames");
}

#[test]
fn missing_keyframes_are_the_elements_own_values() {
    let mut page = Page::timed(GROW);
    page.set_time(2000.0);
    page.set_class("t", "half");
    assert_eq!(page.value("t", "width"), "50px");
    page.frame(2250.0);
    assert_eq!(page.value("t", "width"), "75px");
    page.frame(2750.0);
    assert_eq!(page.value("t", "width"), "75px");
}

#[test]
fn animation_none_and_display_none_cancel() {
    let mut page = Page::timed(GROW);
    page.frame(250.0);
    page.set_class("t", "none");
    assert_eq!(page.value("t", "width"), "50px");
    // Canceled: idle, no longer style's (script may still play it).
    assert!(!page.has_style_animations("t"));
    // Back: a new animation, from its start.
    page.set_class("t", "");
    assert_eq!(page.value("t", "width"), "0px");
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "25px");
    page.set_class("t", "hidden");
    assert!(!page.animations_on("t", None));
}

/// An element in the shadow tree of a `display: none` host isn't
/// rendered: its animation starts once the host is shown.
#[test]
fn a_hidden_hosts_shadow_tree_doesnt_animate() {
    let mut page =
        Page::timed("<style>#h.hidden { display: none }</style><div id=h class=hidden></div>");
    page.with("h", |cx, host| {
        attach_shadow(
            cx,
            host,
            "<style>@keyframes grow { from { width: 0px } to { width: 100px } }
           #s { width: 50px; animation: grow 1s linear }</style><div id=s></div>",
        );
    });
    page.restyle();
    assert!(!page.animations_on("s", None));
    page.set_class("h", "");
    assert!(page.animations_on("s", None));
}

/// What is assigned to a `display: none` slot isn't rendered: its
/// animation starts once the slot is shown.
#[test]
fn what_a_hidden_slot_takes_doesnt_animate() {
    let mut page = Page::timed(
        "<style>@keyframes grow { from { width: 0px } to { width: 100px } }
           #t { width: 50px; animation: grow 1s linear }</style>
         <div id=h><div id=t></div></div>",
    );
    page.with("h", |cx, host| {
        attach_shadow(
            cx,
            host,
            "<style>slot.hidden { display: none }</style><slot id=s class=hidden></slot>",
        );
    });
    page.restyle();
    assert!(!page.animations_on("t", None));
    page.set_class("s", "");
    assert!(page.animations_on("t", None));
}

/// Whether an element is rendered, carried down style's walk, is what
/// walking up its flat-tree ancestors finds (which a debug build checks
/// for every element): for a slot's fallback children while something is
/// assigned to it, which have no flat-tree parent.
#[test]
fn rendered_is_carried_down_as_walking_up_finds_it() {
    let mut page = Page::timed("<div id=h><div></div></div>");
    page.with("h", |cx, host| {
        attach_shadow(
            cx,
            host,
            "<style>slot { display: none }
           #f { animation: grow 1s linear }
           @keyframes grow { from { width: 0px } to { width: 100px } }</style>
         <slot><div id=f></div></slot>",
        );
    });
    page.restyle();
}

/// Only an element removed from its tree is recorded as such (its style
/// is from before): a text node moved back in isn't, so it isn't missed
/// (a debug build checks each one back in the document was styled again);
/// an element moved back styles anew, with no transition from before.
#[test]
fn what_moves_back_in_is_styled_anew() {
    let mut page = Page::timed(GROW);
    page.frame(250.0);
    let words = page.with("t", |cx, element| {
        let body = cx.parent(element).unwrap();
        let words = cx.create_text(cx.document_of(element), "words");
        cx.append_child(body, words).unwrap();
        cx.remove(words);
        cx.append_child(body, words).unwrap();
        cx.remove(element);
        cx.append_child(body, element).unwrap();
        elise::Persistent::new(cx.gc, words)
    });
    page.restyle();
    let removed = page.with("t", |cx, element| {
        cx.removed_since_styled(element) || cx.removed_since_styled(words.get(cx.gc))
    });
    assert!(!removed);
}

/// What is under an element removed and put back (its child here, which
/// the walk knows from its parent) doesn't transition from its style from
/// before; an element not moved does.
#[test]
fn what_is_under_an_element_moved_back_doesnt_transition() {
    let mut page = Page::timed(
        "<style>.t { width: 10px; transition: width 100s linear } .wide .t { width: 110px }</style>
         <div id=p><div id=c class=t></div></div><div id=q><div id=d class=t></div></div>",
    );
    page.set_time(0.0);
    page.set_class("q", "wide");
    page.frame(50_000.0);
    assert_eq!(page.value("d", "width"), "60px", "not moved: transitions");
    page.with("p", |cx, p| {
        cx.set_attribute(p, "class", "wide");
        let body = cx.parent(p).unwrap();
        cx.remove(p);
        cx.append_child(body, p).unwrap();
    });
    page.restyle();
    assert_eq!(page.value("c", "width"), "110px", "moved: no transition");
}

/// Nor what is in the shadow tree of a host removed and put back.
#[test]
fn what_is_in_a_shadow_tree_moved_back_doesnt_transition() {
    let mut page = Page::timed(
        "<style>.t { width: 10px; transition: width 100s linear } .wide .t { width: 110px }</style>
         <div id=host></div><div id=q><div id=d class=t></div></div>",
    );
    page.with("host", |cx, host| {
        attach_shadow(
            cx,
            host,
        "<style>.t { width: 10px; transition: width 100s linear } :host(.wide) .t { width: 110px }</style>\
         <div id=s class=t></div>",
        );
    });
    page.restyle();
    page.set_time(0.0);
    page.set_class("q", "wide");
    page.frame(50_000.0);
    assert_eq!(page.value("d", "width"), "60px", "not moved: transitions");
    page.with("host", |cx, host| {
        cx.set_attribute(host, "class", "wide");
        let body = cx.parent(host).unwrap();
        cx.remove(host);
        cx.append_child(body, host).unwrap();
    });
    page.restyle();
    assert_eq!(page.value("s", "width"), "110px", "moved: no transition");
}

#[test]
fn no_keyframes_no_animation() {
    let mut page = Page::timed(
        "<style>#t { width: 5px; animation: nothing 1s } #t.grow { animation: grow 1s linear }
         @keyframes grow { to { width: 105px } }</style><div id=t></div>",
    );
    assert!(!page.animations_on("t", None));
    page.set_time(100.0);
    page.set_class("t", "grow");
    page.frame(600.0);
    assert_eq!(page.value("t", "width"), "55px");
}

#[test]
fn delay_and_fill() {
    let mut page = Page::timed(
        "<style>@keyframes grow { from { width: 10px } to { width: 110px } }
         #t { width: 0px; animation: grow 1s linear 500ms }
         #t.backwards { animation-fill-mode: backwards }
         #t.forwards { animation-fill-mode: forwards }
         #t.both { animation-fill-mode: both }
         #t.negative { animation-delay: -250ms }
         #t.none { animation: none }</style><div id=t></div>",
    );
    assert_eq!(
        page.value("t", "width"),
        "0px",
        "no fill: nothing in the delay"
    );
    page.frame(1000.0);
    assert_eq!(page.value("t", "width"), "60px");
    page.frame(1500.0);
    assert_eq!(page.value("t", "width"), "0px", "nothing after");
    page.set_class("t", "forwards");
    assert_eq!(page.value("t", "width"), "110px", "its last keyframe after");
    // Ticked, done (no frames), and styled again for something else: it
    // still fills.
    page.frame(1600.0);
    page.set_class("t", "forwards other");
    assert_eq!(page.value("t", "width"), "110px");
    page.set_class("t", "both");
    assert_eq!(page.value("t", "width"), "110px");
    page.set_class("t", "none");
    page.set_class("t", "backwards");
    assert_eq!(
        page.value("t", "width"),
        "10px",
        "its first keyframe in the delay"
    );
    page.set_class("t", "none");
    page.set_time(2000.0);
    page.set_class("t", "negative");
    assert_eq!(page.value("t", "width"), "35px", "a quarter in already");
}

#[test]
fn iterations_and_directions() {
    let mut page = Page::timed(
        "<style>@keyframes grow { from { width: 0px } to { width: 100px } }
         #t { animation: grow 1s linear 2 alternate }
         #t.reverse { animation: grow 1s linear infinite reverse }
         #t.alternate-reverse { animation: grow 1s linear 1.5 alternate-reverse forwards }
         #t.none { animation: none }</style><div id=t></div>",
    );
    page.frame(250.0);
    assert_eq!(page.value("t", "width"), "25px");
    page.frame(1250.0);
    assert_eq!(page.value("t", "width"), "75px", "back in the second");
    page.set_class("t", "none");
    page.set_time(5000.0);
    page.set_class("t", "reverse");
    page.frame(5250.0);
    assert_eq!(page.value("t", "width"), "75px");
    page.frame(9250.0);
    assert_eq!(page.value("t", "width"), "75px", "infinite");
    page.set_class("t", "none");
    page.set_time(10000.0);
    page.set_class("t", "alternate-reverse");
    page.frame(10250.0);
    assert_eq!(page.value("t", "width"), "75px");
    page.frame(11250.0);
    assert_eq!(page.value("t", "width"), "25px");
    page.frame(12000.0);
    assert_eq!(page.value("t", "width"), "50px", "filling where it stopped");
}

#[test]
fn each_keyframe_eases_to_the_next() {
    let mut page = Page::timed(
        "<style>@keyframes k {
           from { width: 0px; height: 0px; animation-timing-function: steps(2, end) }
           50% { height: 50px }
           to { width: 100px; height: 100px } }
         #t { animation: k 1s steps(4, end) }</style><div id=t></div>",
    );
    page.frame(300.0);
    // `width`: its own steps; `height`: the animation's, 0% to 50%.
    assert_eq!(page.value("t", "width"), "0px");
    assert_eq!(page.value("t", "height"), "25px");
    // `animation-timing-function` in a keyframe is its easing, not
    // animated (its value would be the first keyframe's here).
    assert_eq!(page.value("t", "animation-timing-function"), "steps(4)");
    page.frame(600.0);
    assert_eq!(page.value("t", "width"), "50px");
    // 50% to 100%: the 50% keyframe's easing is the animation's.
    assert_eq!(page.value("t", "height"), "50px");
    page.frame(800.0);
    assert_eq!(page.value("t", "height"), "75px");
}

#[test]
fn the_easing_is_the_keyframes_only() {
    let mut page = Page::timed(
        "<style>@keyframes grow { from { width: 0px } to { width: 100px } }
         #t { animation: grow 1s ease-in }</style><div id=t></div>",
    );
    page.frame(500.0);
    // `ease-in` at 0.5 once: 0.3154 (twice would be about 0.12).
    let width = page.value("t", "width");
    let px: f64 = width.trim_end_matches("px").parse().unwrap();
    assert!((px - 31.54).abs() < 0.01, "{width}");
}

#[test]
fn a_step_easing_in_the_delay_is_before() {
    let mut page = Page::timed(
        "<style>@keyframes grow { from { width: 0px } to { width: 100px } }
         #t { animation: grow 1s steps(1, start) 1s backwards }</style><div id=t></div>",
    );
    assert_eq!(page.value("t", "width"), "0px");
    page.frame(1500.0);
    assert_eq!(page.value("t", "width"), "100px");
}

#[test]
fn a_done_animation_made_longer_runs_again() {
    let mut page = Page::timed(
        "<style>@keyframes grow { from { width: 0px } to { width: 100px } }
         #t { width: 5px; animation: grow 1s linear }
         #t.three { animation-iteration-count: 3 }</style><div id=t></div>",
    );
    page.frame(1500.0);
    assert_eq!(page.value("t", "width"), "5px");
    page.set_class("t", "three");
    assert_eq!(page.value("t", "width"), "50px");
    page.frame(1750.0);
    assert_eq!(page.value("t", "width"), "75px");
}

#[test]
fn keyframes_cascade_and_the_last_one_wins() {
    let mut page = Page::timed(
        "<style>@keyframes k {
           100% { margin-inline-start: 30px }
           0%, 100% { width: 10px; height: 10px }
           100% { width: 30px }
           50% { color: blue }
           0% { margin-left: 10px } }
         #t { animation: k 1s linear; color: red }</style><div id=t></div>",
    );
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "20px");
    assert_eq!(page.value("t", "height"), "10px");
    // Out of order, and a flow-relative one as its physical one.
    assert_eq!(page.value("t", "margin-left"), "20px");
    assert_eq!(page.value("t", "color"), "rgb(0, 0, 255)");
    page.frame(250.0 + 500.0);
    assert_eq!(page.value("t", "color"), "rgb(128, 0, 128)");
}

#[test]
fn keyframes_are_computed_on_the_element() {
    let mut page = Page::timed(
        "<style>@keyframes k { to { width: var(--w); height: 2em; margin-left: inherit } }
         #p { margin-left: 40px }
         #p.far { margin-left: 80px }
         #t { --w: 40px; font-size: 10px; width: 0px; height: 0px; animation: k 1s linear }
         #t.wider { --w: 80px }</style><div id=p><div id=t></div></div>",
    );
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "20px");
    assert_eq!(page.value("t", "height"), "10px");
    assert_eq!(page.value("t", "margin-left"), "20px");
    let before = effects(&mut page);
    // A restyle that changes nothing keeps the keyframes.
    page.set_class("t", "other");
    assert!(Arc::ptr_eq(&before[0], &effects(&mut page)[0]));
    assert_eq!(page.value("t", "width"), "20px");
    // A new base style computes them again.
    page.set_class("t", "wider");
    assert!(!Arc::ptr_eq(&before[0], &effects(&mut page)[0]));
    assert_eq!(page.value("t", "width"), "40px");
    // A new parent style too.
    page.set_class("p", "far");
    assert_eq!(page.value("t", "margin-left"), "40px");
}

#[test]
fn keyframes_follow_the_root_font_size_and_the_viewport() {
    let mut page = Page::timed(
        "<html id=r><style>@keyframes k { to { width: 4rem; height: 10vw } }
         #r.big { font-size: 20px }
         body { font-size: 16px }
         #t { font-size: 10px; width: 0px; height: 0px; animation: k 1s linear }</style>\
         <div id=t></div></html>",
    );
    page.set_viewport(gfx::SizeF::new(1000.0, 500.0), 1.0);
    page.restyle();
    page.set_class("t", "x");
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "32px");
    assert_eq!(page.value("t", "height"), "50px");
    page.set_class("r", "big");
    assert_eq!(page.value("t", "width"), "40px");
    page.set_viewport(gfx::SizeF::new(500.0, 500.0), 1.0);
    page.restyle();
    assert_eq!(page.value("t", "height"), "25px");
}

/// A finished animation filling forwards with a viewport unit follows a
/// new viewport, though the element's own style has none (WPT's
/// `fill-forwards-viewport-units`).
#[test]
fn a_filled_viewport_unit_follows_the_viewport() {
    let mut page = Page::timed(
        "<style>@keyframes k { to { width: 50vw } }
         #t { width: 0px; animation: k 1s linear forwards }</style><div id=t></div>",
    );
    page.set_viewport(gfx::SizeF::new(800.0, 500.0), 1.0);
    page.restyle();
    page.frame(1500.0);
    assert_eq!(page.value("t", "width"), "400px");
    page.set_viewport(gfx::SizeF::new(400.0, 500.0), 1.0);
    page.restyle();
    assert_eq!(page.value("t", "width"), "200px");
}

#[test]
fn important_beats_animations_and_animations_beat_normal() {
    let mut page = Page::timed(
        "<style>@keyframes k { from { width: 0px; height: 0px } to { width: 100px; height: 100px } }
         #t { width: 7px !important; height: 7px; animation: k 1s linear }</style>\
         <div id=t style='height: 9px'></div>",
    );
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "7px");
    assert_eq!(
        page.value("t", "height"),
        "50px",
        "over a style attribute too"
    );
}

#[test]
fn an_animated_property_doesnt_transition() {
    let mut page = Page::timed(
        "<style>@keyframes grow { from { width: 0px } to { width: 100px } }
         #t { width: 0px; transition: all 1s linear; animation: grow 2s linear }
         #t.red { color: red }</style><div id=t></div>",
    );
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "25px");
    // The before-change and after-change styles both have its value.
    page.set_class("t", "red");
    assert_eq!(page.value("t", "width"), "25px");
    page.frame(1000.0);
    assert_eq!(page.value("t", "width"), "50px");
}

#[test]
fn a_new_name_list_keeps_the_animations_it_still_names() {
    let mut page = Page::timed(
        "<style>@keyframes a { from { width: 0px } to { width: 100px } }
         @keyframes b { from { height: 0px } to { height: 100px } }
         #t { animation: a 1s linear }
         #t.both { animation: a 2s linear, b 1s linear }
         #t.b { animation: b 1s linear }</style><div id=t></div>",
    );
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "50px");
    page.set_class("t", "both");
    // `a` keeps its start, with its new duration; `b` starts now.
    assert_eq!(page.value("t", "width"), "25px");
    assert_eq!(page.value("t", "height"), "0px");
    page.frame(1000.0);
    assert_eq!(page.value("t", "width"), "50px");
    assert_eq!(page.value("t", "height"), "50px");
    page.set_class("t", "b");
    assert_eq!(page.value("t", "width"), "auto", "`a` canceled");
    assert_eq!(page.value("t", "height"), "50px", "`b` kept, moved first");
}

/// A name given twice: matched from the ends, the running animation is
/// the last one, and a new one goes before it (under it, in composite
/// order); one of them taken away, the first goes.
#[test]
fn the_last_of_a_name_keeps_the_last_animation() {
    let mut page = Page::timed(
        "<style>@keyframes a { from { width: 0px } to { width: 100px } }
         #t { animation: a 4s linear }
         #t.two { animation: a 4s linear, a 4s linear }</style><div id=t></div>",
    );
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "12.5px");
    page.set_class("t", "two");
    assert_eq!(page.value("t", "width"), "12.5px", "the old one on top");
    page.frame(1500.0);
    assert_eq!(page.value("t", "width"), "37.5px");
    page.set_class("t", "");
    assert_eq!(page.value("t", "width"), "37.5px", "the new one gone");
}

#[test]
fn paused_holds_where_it_is() {
    let mut page = Page::timed(
        "<style>@keyframes grow { from { width: 0px } to { width: 100px } }
         #t { animation: grow 1s linear }
         #t.paused { animation-play-state: paused }</style><div id=t></div>",
    );
    page.frame(250.0);
    page.set_class("t", "paused");
    page.frame(5000.0);
    assert_eq!(page.value("t", "width"), "25px");
    page.set_class("t", "");
    assert_eq!(page.value("t", "width"), "25px");
    page.frame(5250.0);
    assert_eq!(page.value("t", "width"), "50px", "on from where it was");
}

#[test]
fn paused_from_the_start() {
    let mut page = Page::timed(
        "<style>@keyframes grow { from { width: 10px } to { width: 110px } }
         #t { animation: grow 1s linear paused }</style><div id=t></div>",
    );
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "10px");
    let running = page.with_document(|cx, document| {
        cx.elements_with_animations(document)
            .into_iter()
            .any(|element| {
                cx.read_animations(element, |targets| {
                    targets
                        .iter()
                        .flatten()
                        .any(|target| target.animations.is_running())
                })
                .unwrap_or(false)
            })
    });
    assert!(!running);
}

#[test]
fn descendants_and_ems_follow() {
    let mut page = Page::timed(
        "<style>@keyframes k { from { color: rgb(0, 0, 0); font-size: 10px }
                               to { color: rgb(200, 0, 0); font-size: 30px } }
         #t { animation: k 1s linear; width: 2em }</style>\
         <div id=t><span id=child></span></div>",
    );
    page.frame(500.0);
    assert_eq!(page.value("t", "font-size"), "20px");
    assert_eq!(page.value("t", "width"), "40px");
    assert_eq!(page.value("child", "color"), "rgb(100, 0, 0)");
    assert_eq!(page.value("child", "font-size"), "20px");
}

#[test]
fn pseudo_elements_animate() {
    let mut page = Page::timed(
        "<style>@keyframes grow { from { width: 0px } to { width: 100px } }
         #t::before { content: ''; display: block; animation: grow 1s linear }</style>\
         <div id=t></div>",
    );
    assert!(page.animations_on("t", Some(computed_style::PseudoId::Before)));
    page.frame(500.0);
    assert_eq!(
        page.pseudo_value("t", computed_style::PseudoId::Before, "width"),
        "50px"
    );
}

#[test]
fn a_shadow_trees_keyframes() {
    let mut page = Page::timed("<div id=host></div>");
    page.with("host", |cx, host| {
        attach_shadow(
            cx,
            host,
        "<style>@keyframes grow { to { width: 100px } } #t { width: 0px; animation: grow 1s linear }\
         </style><div id=t></div>",
        );
    });
    page.restyle();
    page.frame(500.0);
    assert_eq!(page.value("t", "width"), "50px");
}

#[test]
fn events_as_the_phase_changes() {
    let mut page = Page::timed(
        "<style>@keyframes k { to { width: 100px } }
         #t { animation: k 100ms linear 500ms 3 }</style><div id=t></div>",
    );
    page.frame(100.0);
    assert_eq!(events(&mut page), Vec::<String>::new(), "in its delay");
    page.frame(550.0);
    assert_eq!(events(&mut page), ["animationstart k 0@500"]);
    page.frame(580.0);
    assert_eq!(events(&mut page), Vec::<String>::new());
    // Two iterations in one frame: one event, of the last.
    page.frame(780.0);
    assert_eq!(events(&mut page), ["animationiteration k 0.2@700"]);
    page.frame(900.0);
    assert_eq!(events(&mut page), ["animationend k 0.3@800"]);
    page.frame(950.0);
    assert_eq!(events(&mut page), Vec::<String>::new());
}

#[test]
fn start_and_end_in_one_frame() {
    let mut page = Page::timed(
        "<style>@keyframes k { to { width: 100px } }
         #t { animation: k 100ms 50ms }</style><div id=t></div>",
    );
    page.frame(500.0);
    assert_eq!(
        events(&mut page),
        ["animationstart k 0@50", "animationend k 0.1@150"]
    );
}

#[test]
fn a_negative_delay_starts_partway() {
    let mut page = Page::timed(
        "<style>@keyframes k { to { width: 100px } }
         #t { animation: k 1s -250ms }</style><div id=t></div>",
    );
    page.frame(16.0);
    assert_eq!(events(&mut page), ["animationstart k 0.25@0"]);
}

#[test]
fn a_paused_one_starts_where_it_holds() {
    let mut page = Page::timed(
        "<style>@keyframes k { to { width: 100px } }
         #t { animation: k 1s paused }
         #t.later { animation-delay: 1s }</style><div id=t></div>",
    );
    page.frame(16.0);
    assert_eq!(events(&mut page), ["animationstart k 0@16"]);
    page.frame(32.0);
    assert_eq!(events(&mut page), Vec::<String>::new());
    // Back into its delay: it ended, backwards.
    page.set_class("t", "later");
    page.frame(48.0);
    assert_eq!(events(&mut page), ["animationend k 0@48"]);
}

#[test]
fn a_new_timing_doesnt_start_it_again() {
    let mut page = Page::timed(
        "<style>@keyframes k { to { width: 100px } }
         #t { animation: k 1s }
         #t.longer { animation-duration: 2s }
         #t.short { animation-duration: 100ms }
         #t.later { animation-duration: 100ms; animation-delay: 5s }</style><div id=t></div>",
    );
    page.frame(100.0);
    assert_eq!(events(&mut page), ["animationstart k 0@0"]);
    page.set_class("t", "longer");
    page.frame(200.0);
    assert_eq!(events(&mut page), Vec::<String>::new());
    // Done, then longer: active again.
    page.set_class("t", "short");
    page.frame(300.0);
    assert_eq!(events(&mut page), ["animationend k 0.1@100"]);
    // Back from its end: the interval's end is the start's elapsed time.
    page.set_class("t", "longer");
    page.frame(400.0);
    assert_eq!(events(&mut page), ["animationstart k 2@400"]);
    // Done again, then back before its start: started and ended at once,
    // backwards.
    page.set_class("t", "short");
    page.frame(500.0);
    assert_eq!(events(&mut page), ["animationend k 0.1@100"]);
    page.set_class("t", "later");
    page.frame(600.0);
    assert_eq!(
        events(&mut page),
        ["animationstart k 0.1@600", "animationend k 0@600"]
    );
}

#[test]
fn canceled_ones_say_so_unless_done() {
    let mut page = Page::timed(
        "<style>@keyframes k { to { width: 100px } }
         #t { animation: k 1s, j 100ms }
         @keyframes j { to { height: 100px } }
         #t.k { animation: k 1s }
         #t.none { animation: none }
         #t.waiting { animation: k 1s 5s }</style><div id=t></div>",
    );
    page.frame(400.0);
    assert_eq!(
        events(&mut page),
        [
            "animationstart k 0@0",
            "animationstart j 0@0",
            "animationend j 0.1@100"
        ]
    );
    // `j` was done: no cancel.
    page.set_class("t", "k");
    assert_eq!(events(&mut page), Vec::<String>::new());
    page.set_class("t", "none");
    assert_eq!(events(&mut page), ["animationcancel k 0.4@400"]);
    // In its delay, it had started (not idle): canceled at 0.
    page.set_class("t", "waiting");
    page.frame(500.0);
    page.set_class("t", "none");
    assert_eq!(events(&mut page), ["animationcancel k 0@500"]);
    // Never ticked: nothing.
    page.set_class("t", "k");
    page.set_class("t", "none");
    assert_eq!(events(&mut page), Vec::<String>::new());
}

#[test]
fn display_none_cancels_with_an_event() {
    let mut page = Page::timed(GROW);
    page.frame(300.0);
    events(&mut page);
    page.set_class("t", "hidden");
    assert_eq!(events(&mut page), ["animationcancel grow 0.3@300"]);
}

#[test]
fn a_markers_animation() {
    let mut page = Page::timed(
        "<style>@keyframes k { from { color: rgb(0, 0, 0) } to { color: rgb(200, 0, 0) } }
         #t { display: list-item }
         #t::marker { animation: k 1s linear }</style><div id=t>x</div>",
    );
    page.frame(500.0);
    assert_eq!(
        page.pseudo_value("t", computed_style::PseudoId::Marker, "color"),
        "rgb(100, 0, 0)"
    );
}
