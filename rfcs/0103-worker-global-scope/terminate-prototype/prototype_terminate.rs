//! PROTOTYPE (RFC 0103, 1b; not to be merged): script on an engine of a
//! thread of its own, stopped from another thread while it calls the DOM,
//! at many points of its calls; then the engine dropped on its thread.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

fn run(engine: &mut V8Engine, source: &str) {
    engine.run_classic_script(
        engine.top_frame(),
        &ClassicScript {
            source,
            url: "https://example.test/worker.js",
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        },
    );
}

/// Busy loops that keep calling into the DOM: making objects, strings both
/// ways, events dispatched to listeners (which call script back from the
/// bindings), exceptions thrown and caught, promises, timers.
const LOOPS: &[&str] = &[
    "for (;;) { const e = new Event('x'); e.type; e.timeStamp; }",
    "for (;;) { const d = document.createElement('div'); d.id = 'a' + Math.random(); d.append(document.createTextNode(d.id)); document.body.append(d); d.remove(); }",
    "const t = new EventTarget(); t.addEventListener('x', e => { new CustomEvent('y', { detail: e }); }); for (;;) t.dispatchEvent(new Event('x'));",
    "for (;;) { try { document.createElement('1'); } catch (e) { e.name; } }",
    "for (;;) { const c = new AbortController(); c.signal.onabort = () => {}; c.abort('why'); AbortSignal.any([c.signal]); }",
    "for (;;) { new URL('a?b=' + Math.random(), 'https://example.test/').searchParams.get('b'); new Request('/x'); new Response('y'); }",
    "for (;;) { setTimeout(() => {}, 1); queueMicrotask(() => {}); Promise.resolve(1).then(x => x); }",
    "for (;;) { const ch = new MessageChannel(); ch.port1.postMessage({ a: [1, 'b'] }); ch.port2.start(); }",
    "for (;;) { document.body.innerHTML = '<p>a<b>c</b></p>'.repeat(10); document.querySelectorAll('b').length; }",
];

#[test]
fn script_stopped_from_another_thread_while_it_calls_the_dom() {
    let started = Instant::now();
    let mut runs = 0;
    let mut stuck = Vec::new();
    let mut panicked = Vec::new();
    for (index, source) in LOOPS.iter().enumerate() {
        for delay_us in [0u64, 50, 200, 1_000, 5_000, 20_000] {
            for _ in 0..3 {
                let (tx, rx) = mpsc::channel();
                let (done_tx, done_rx) = mpsc::channel();
                let source = (*source).to_owned();
                let worker = std::thread::Builder::new()
                    .name("PrototypeWorker".into())
                    .stack_size(8 << 20)
                    .spawn(move || {
                        let mut engine = V8Engine::new(EngineSettings { web_tests: true });
                        engine
                            .create_window(engine.top_frame(), "https://example.test/", Instant::now())
                            .expect("a window");
                        tx.send(engine.prototype_terminator()).unwrap();
                        run(&mut engine, &source);
                        done_tx.send(()).unwrap();
                        engine.run_due_tasks();
                        drop(engine);
                    })
                    .unwrap();
                let terminate = rx.recv().unwrap();
                std::thread::sleep(Duration::from_micros(delay_us));
                terminate();
                runs += 1;
                if done_rx.recv_timeout(Duration::from_secs(3)).is_err() {
                    // Left running: its thread can't be stopped.
                    stuck.push((index, delay_us));
                    break;
                }
                if worker.join().is_err() {
                    panicked.push((index, delay_us));
                }
            }
        }
    }
    eprintln!("PROTOTYPE {runs} runs in {:?}; stuck {stuck:?}; panicked {panicked:?}", started.elapsed());
}