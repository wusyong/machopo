//! Web Animations from script, in a frame whose layout styles nothing (the
//! engine's frames driven by the test: a frame showing the animations
//! starts them, the update of animations moves them on): `animate()`'s
//! arguments and its animation's members, `getAnimations()`, the
//! `ready` and `finished` promises (the same until replaced, settled as
//! the playback goes), `finish` and `cancel` events, `new Animation()`,
//! `AnimationPlaybackEvent`; `KeyframeEffect`, effects given and taken,
//! `persist()`, `commitStyles()`'s errors.

use std::time::{Duration, Instant};

use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

/// A frame's layout that styles nothing.
struct Plain(dom::NodeId);

impl dom_api::FrameLayout for Plain {
    fn shows(&mut self, document: dom::NodeId) -> bool {
        document == self.0
    }
    fn update_style_and_layout(&mut self, _: &dom::Cx<'_, '_>) {}
    fn element_box(&self, _: dom::NodeId) -> Option<gfx::ElementBox> {
        None
    }
    fn client_rects(&self, _: dom::NodeId) -> Vec<gfx::RectF> {
        Vec::new()
    }
    fn viewport(&self) -> gfx::Viewport {
        gfx::Viewport {
            size: gfx::SizeF::new(100.0, 100.0),
            device_scale_factor: 1.0,
            scroll: gfx::PointF::new(0.0, 0.0),
            content_size: gfx::SizeF::new(100.0, 100.0),
        }
    }
    fn window_screen_rect(&self) -> Option<gfx::RectF> {
        None
    }
    fn computed_style<'gc>(
        &self,
        _: &dom::Cx<'_, 'gc>,
        _: dom::NodeRef<'gc>,
    ) -> Option<std::sync::Arc<computed_style::ComputedStyle>> {
        None
    }
    fn element_at(&mut self, _: gfx::PointF) -> Option<dom::NodeId> {
        None
    }
    fn elements_at(&mut self, _: gfx::PointF) -> Vec<dom::NodeId> {
        Vec::new()
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

struct Page {
    engine: V8Engine,
    start: Instant,
}

impl Page {
    fn new() -> Self {
        let mut engine = V8Engine::new(EngineSettings { web_tests: true });
        let start = Instant::now();
        let document = engine
            .create_window(engine.top_frame(), "https://example.test/", start)
            .expect("a window");
        let id = engine.dom_mut().read(|cx| cx.node_id(document.get(cx.gc)));
        drop(document);
        engine.set_layout(Box::new(Plain(id)));
        let mut page = Self { engine, start };
        page.eval("document.appendChild(document.createElement('html')); var e = document.documentElement; 0");
        page
    }

    /// `source`'s completion value as a string, or the name of what it
    /// throws, `!` first; then a microtask checkpoint.
    fn eval(&mut self, source: &str) -> String {
        let source = format!(
            "testRunner.reportHarnessResults((() => {{ try {{ return String((0, eval)({source:?})); }} \
             catch (e) {{ return '!' + e.name; }} }})())"
        );
        self.engine.run_classic_script(
            self.engine.top_frame(),
            &ClassicScript {
                source: &source,
                url: "https://example.test/",
                base_url: None,
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
        self.engine.perform_microtask_checkpoint();
        self.engine.take_harness_results().expect("reported")
    }

    /// A frame at `ms` milliseconds from the window's start: its update of
    /// animations, then (its rendering shown) the pending ones started.
    fn frame(&mut self, ms: u64) {
        let frame = self.engine.top_frame();
        let at = self.start + Duration::from_millis(ms);
        self.engine.update_animations(frame, at);
        self.engine.start_pending_animations(frame);
    }
}

#[test]
fn animate_and_its_animations_members() {
    let mut page = Page::new();
    assert_eq!(
        page.eval(
            "var a = e.animate({ opacity: [0, 1] }, { duration: 100, id: 'fade' }); \
             [a instanceof Animation, a.id, a.playState, a.pending, a.startTime, \
              a.timeline === document.timeline, a.effect instanceof KeyframeEffect, a.effect === a.effect, \
              a.playbackRate].join()"
        ),
        "true,fade,running,true,,true,true,true,1"
    );
    assert_eq!(
        page.eval("[e.getAnimations()[0] === a, document.getAnimations()[0] === a, e.getAnimations().length].join()"),
        "true,true,1"
    );
    page.frame(10);
    assert_eq!(
        page.eval("[a.pending, a.startTime !== null].join()"),
        "false,true"
    );
    page.frame(60);
    assert_eq!(page.eval("String(Math.round(a.currentTime))"), "50");
    assert_eq!(page.eval("a.pause(); a.playState"), "paused");
    assert_eq!(
        page.eval("a.currentTime = 20; Math.round(a.currentTime)"),
        "20"
    );
    assert_eq!(
        page.eval("a.currentTime = null"),
        "!TypeError",
        "it has one"
    );
    assert_eq!(page.eval("a.playbackRate = 2; a.playbackRate"), "2");
    // Pending rates: applied once a frame shows them.
    assert_eq!(page.eval("a.reverse(); a.playbackRate"), "2");
    page.frame(70);
    assert_eq!(page.eval("a.playbackRate"), "-2");
    assert_eq!(page.eval("a.updatePlaybackRate(3); a.playbackRate"), "-2");
    page.frame(80);
    assert_eq!(page.eval("a.playbackRate"), "3");
    // A number is the duration; `undefined` no keyframes.
    assert_eq!(page.eval("e.animate(null, 10).effect !== null"), "true");
    // The arguments' checks.
    for (source, error) in [
        ("e.animate({ opacity: 1 }, -1)", "!TypeError"),
        (
            "e.animate({ opacity: 1 }, { easing: 'nope' })",
            "!TypeError",
        ),
        ("e.animate({ opacity: 1 }, { fill: 'all' })", "!TypeError"),
        (
            "e.animate({ opacity: 1 }, { direction: 'up' })",
            "!TypeError",
        ),
        ("e.animate({ opacity: 1 }, { delay: NaN })", "!TypeError"),
        ("e.animate([{ offset: 2, opacity: 1 }])", "!TypeError"),
        (
            "e.animate({ opacity: 1 }, { pseudoElement: 'x' })",
            "!SyntaxError",
        ),
        ("e.animate({ opacity: 1 }, { timeline: {} })", "!TypeError"),
        ("e.animate(1)", "!TypeError"),
    ] {
        assert_eq!(page.eval(source), error, "{source}");
    }
    assert_eq!(
        page.eval("e.animate({ opacity: 1 }, { timeline: null }).timeline"),
        "null"
    );
}

#[test]
fn ready_and_finished_settle_as_it_plays() {
    let mut page = Page::new();
    page.eval(
        "var log = []; var a = e.animate({ opacity: [0, 1] }, 50); var r = a.ready; \
         var f = a.finished; r.then((x) => log.push('ready ' + (x === a))); \
         f.then((x) => log.push('finished ' + (x === a))); \
         a.onfinish = (event) => log.push('finish ' + event.currentTime + ' ' + (event instanceof AnimationPlaybackEvent)); 0",
    );
    assert_eq!(
        page.eval("String(a.ready === r && a.finished === f)"),
        "true",
        "the same each time"
    );
    page.frame(5);
    assert_eq!(
        page.eval("log.join()"),
        "ready true",
        "once a frame showed it"
    );
    page.frame(100);
    assert_eq!(
        page.eval("log.join()"),
        "ready true,finished true,finish 50 true",
        "the promise before the event"
    );
    // Played again: a new `finished`; canceled: it rejects, `cancel` fires.
    assert_eq!(page.eval("a.play(); String(a.finished !== f)"), "true");
    page.eval(
        "a.oncancel = () => log.push('cancel'); a.finished.catch((x) => log.push(x.name)); a.cancel(); 0",
    );
    page.frame(110);
    assert_eq!(page.eval("log.slice(3).join()"), "AbortError,cancel");
    assert_eq!(page.eval("a.playState"), "idle");
    // `finish()` settles at once (its microtask).
    assert_eq!(
        page.eval("var b = e.animate(null, 1000); b.finish(); var d = 'no'; b.finished.then(() => { d = 'yes'; }); 0"),
        "0"
    );
    assert_eq!(page.eval("d"), "yes");
}

#[test]
fn new_animation_and_playback_events() {
    let mut page = Page::new();
    assert_eq!(
        page.eval(
            "var n = new Animation(); [n.playState, n.effect, n.timeline === document.timeline, \
             new Animation(null, null).timeline].join()"
        ),
        "idle,,true,"
    );
    // An effect taken from another animation: it leaves it.
    assert_eq!(
        page.eval("var a = e.animate(null, 100); var t = new Animation(a.effect); [t.effect !== null, a.effect].join()"),
        "true,"
    );
    assert_eq!(page.eval("new Animation(null, {})"), "!TypeError");
    assert_eq!(
        page.eval(
            "var p = new AnimationPlaybackEvent('finish', { currentTime: 5, timelineTime: 7, bubbles: true }); \
             [p.type, p.currentTime, p.timelineTime, p.bubbles, new AnimationPlaybackEvent('x').currentTime].join()"
        ),
        "finish,5,7,true,"
    );
    assert_eq!(page.eval("typeof document.timeline.currentTime"), "number");
}

/// An animation whose element left the document plays on off it (script
/// holds it): `ready`, `cancel()` and its event; one done (no fill) isn't
/// listed; a rejected promise reports no unhandled rejection.
#[test]
fn off_its_element_and_listed_while_relevant() {
    let mut page = Page::new();
    page.eval(
        "var log = []; addEventListener('unhandledrejection', () => log.push('unhandled')); \
         var d = e.appendChild(document.createElement('div')); var a = d.animate(null, 100000); d.remove(); \
         a.oncancel = () => log.push('cancel'); a.ready.then(() => log.push('ready')); 0",
    );
    page.frame(5);
    page.frame(10);
    assert_eq!(page.eval("log.join()"), "ready");
    assert_eq!(
        page.eval("a.playState"),
        "running",
        "off its element, it plays on"
    );
    page.eval("a.cancel(); 0");
    page.frame(20);
    assert_eq!(
        page.eval("log.join()"),
        "ready,cancel",
        "no unhandled rejection of its finished"
    );
    // Done with no fill: not listed.
    page.eval("var b = e.animate(null, 10); 0");
    assert_eq!(page.eval("String(e.getAnimations().length)"), "1");
    page.frame(30);
    page.frame(100);
    assert_eq!(page.eval("String(e.getAnimations().length)"), "0");
    assert_eq!(
        page.eval(
            "e.animate(null, { duration: 10, fill: 'forwards' }); String(e.getAnimations().length)"
        ),
        "1"
    );
}

/// The promises settle before the call that changed them returns (no
/// frame needed), one asked for after its animation's settled is settled
/// at once, and they report no unhandled rejection; an animation off any
/// element finishes as it plays.
#[test]
fn promises_settle_as_calls_return() {
    let mut page = Page::new();
    page.eval(
        "var log = []; addEventListener('unhandledrejection', () => log.push('unhandled')); \
         var a = e.animate(null, 100000); var f = a.finished; var r = a.ready; \
         f.catch((x) => log.push('rejected ' + x.name)); a.cancel(); 0",
    );
    assert_eq!(
        page.eval("log.join()"),
        "rejected AbortError",
        "no frame, and `ready` (rejected too, with no handler) reports nothing"
    );
    // Ready once a frame showed it; asked for only after.
    page.eval("var b = e.animate(null, 100000); 0");
    page.frame(5);
    page.eval("b.ready.then(() => log.push('ready late')); 0");
    assert_eq!(page.eval("log.slice(1).join()"), "ready late");
    // Off any element: it finishes.
    page.eval(
        "var d = e.appendChild(document.createElement('div')); var c = d.animate(null, 20); d.remove(); \
         c.onfinish = () => log.push('finish off'); 0",
    );
    page.frame(10);
    page.frame(15);
    page.frame(100);
    assert_eq!(page.eval("log.slice(2).join()"), "finish off");
}

/// `new KeyframeEffect()`: its timing as written (`getTiming()`, `"auto"`
/// kept), computed (`getComputedTiming()`, none without an animation),
/// `updateTiming()` (checked first: nothing changes if a member fails);
/// its target, pseudo-element and keyframes.
#[test]
fn keyframe_effect_and_its_timing() {
    let mut page = Page::new();
    assert_eq!(
        page.eval(
            "var k = new KeyframeEffect(e, { opacity: [0, 1] }, { duration: 100, delay: 5 }); \
             [k.target === e, k.pseudoElement, k.composite, JSON.stringify(k.getTiming())].join()"
        ),
        r#"true,,replace,{"delay":5,"direction":"normal","duration":100,"easing":"linear","endDelay":0,"fill":"auto","iterationStart":0,"iterations":1}"#
    );
    assert_eq!(
        page.eval("new KeyframeEffect(null, null).getTiming().duration"),
        "auto"
    );
    assert_eq!(
        page.eval("new KeyframeEffect(e, null, 50).getTiming().duration"),
        "50"
    );
    assert_eq!(
        page.eval(
            "var c = new KeyframeEffect(null, null).getComputedTiming(); \
             [c.duration, c.fill, c.localTime, c.progress, c.currentIteration, c.activeDuration, c.endTime].join()"
        ),
        "0,none,,,,0,0"
    );
    assert_eq!(
        page.eval("k.updateTiming({ duration: 200, fill: 'forwards' }); [k.getTiming().duration, k.getTiming().fill].join()"),
        "200,forwards"
    );
    assert_eq!(
        page.eval("k.updateTiming({ delay: 1, duration: -1 })"),
        "!TypeError"
    );
    assert_eq!(
        page.eval("k.updateTiming(); String(k.getTiming().delay)"),
        "5",
        "nothing changed"
    );
    assert_eq!(page.eval("k.getComputedTiming().endTime"), "205");
    for (source, error) in [
        (
            "new KeyframeEffect(e, null, { pseudoElement: 'x' })",
            "!SyntaxError",
        ),
        ("new KeyframeEffect({}, null)", "!TypeError"),
        (
            "new KeyframeEffect(e, null, { duration: 'long' })",
            "!TypeError",
        ),
        ("KeyframeEffect(e, null)", "!TypeError"),
    ] {
        assert_eq!(page.eval(source), error, "{source}");
    }
    // Target and pseudo-element.
    assert_eq!(
        page.eval(
            "k.pseudoElement = '::before'; k.target = null; [k.target, k.pseudoElement].join()"
        ),
        ",::before"
    );
    assert_eq!(page.eval("k.pseudoElement = '::nope'"), "!SyntaxError");
    // Keyframes, as given with their computed offsets.
    assert_eq!(
        page.eval(
            "JSON.stringify(new KeyframeEffect(e, [{ marginLeft: '10px' }, { marginLeft: '20px', offset: 0.5 }, {}])\
             .getKeyframes().map((f) => [f.offset, f.computedOffset, f.easing, f.composite, f.marginLeft]))"
        ),
        r#"[[null,0,"linear","auto","10px"],[0.5,0.5,"linear","auto","20px"],[null,1,"linear","auto",null]]"#
    );
    assert_eq!(
        page.eval("k.setKeyframes({ opacity: [1, 0.5], easing: 'ease-in' }); k.getKeyframes().map((f) => f.opacity + ' ' + f.easing).join()"),
        "1 ease-in,0.5 ease-in"
    );
    assert_eq!(page.eval("k.setKeyframes([{ offset: 2 }])"), "!TypeError");
}

/// An effect given an animation, and taken from it: `new Animation(k)`
/// plays on `k`'s target, `effect = null` leaves it on its object (its
/// timing kept), a target changed moves script's animation; `timeline`
/// set to none and back.
#[test]
fn effects_given_and_taken() {
    let mut page = Page::new();
    page.eval(
        "var k = new KeyframeEffect(e, { opacity: [0, 1] }, { duration: 100, fill: 'forwards' }); \
         var a = new Animation(k); a.play(); 0",
    );
    assert_eq!(
        page.eval("[a.effect === k, e.getAnimations()[0] === a].join()"),
        "true,true"
    );
    page.frame(10);
    page.frame(60);
    assert_eq!(
        page.eval("var t = k.getComputedTiming(); [Math.round(t.localTime), t.progress.toFixed(1), t.currentIteration].join()"),
        "50,0.5,0"
    );
    assert_eq!(
        page.eval("a.effect = null; [a.effect, k.target === e, k.getTiming().fill, e.getAnimations().length].join()"),
        ",true,forwards,0"
    );
    assert_eq!(
        page.eval("String(k.getComputedTiming().localTime)"),
        "null",
        "off its animation"
    );
    assert_eq!(
        page.eval("a.effect = k; [a.effect === k, e.getAnimations()[0] === a].join()"),
        "true,true"
    );
    // Retargeted: script's animation moves to its new target.
    assert_eq!(
        page.eval(
            "var d = e.appendChild(document.createElement('div')); k.target = d; \
             [e.getAnimations().length, d.getAnimations()[0] === a, a.effect.target === d].join()"
        ),
        "0,true,true"
    );
    // An effect another animation has: it leaves that one.
    assert_eq!(
        page.eval("var b = e.animate(null, 100); var bk = b.effect; a.effect = bk; [b.effect, a.effect === bk, k.target === d].join()"),
        ",true,true"
    );
    assert_eq!(page.eval("a.effect = {}"), "!TypeError");
    assert_eq!(page.eval("a.timeline = null; String(a.timeline)"), "null");
    assert_eq!(
        page.eval("a.timeline = document.timeline; a.timeline === document.timeline"),
        "true"
    );
    assert_eq!(page.eval("a.timeline = {}"), "!TypeError");
}

/// `replaceState` and `persist()` (which animations are replaced needs
/// their keyframes computed: through the renderer).
#[test]
fn replace_state_and_persist() {
    let mut page = Page::new();
    assert_eq!(
        page.eval("var x = e.animate(null, 10); var s = x.replaceState; x.persist(); s + ' ' + x.replaceState"),
        "active persisted"
    );
    assert_eq!(page.eval("typeof x.onremove"), "object");
}

/// `commitStyles()` of an element not rendered (this frame's layout
/// styles nothing), or a pseudo-element's: an error, nothing written.
#[test]
fn commit_styles_errors() {
    let mut page = Page::new();
    assert_eq!(
        page.eval("e.animate({ opacity: 0.5 }, 100).commitStyles()"),
        "!InvalidStateError"
    );
    assert_eq!(
        page.eval("e.animate({ opacity: 0.5 }, { duration: 100, pseudoElement: '::before' }).commitStyles()"),
        "!NoModificationAllowedError"
    );
    assert_eq!(
        page.eval("new Animation().commitStyles(); e.getAttribute('style')"),
        "null"
    );
}
