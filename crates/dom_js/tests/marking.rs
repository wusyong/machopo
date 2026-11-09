//! The DOM from JS under V8's incremental markings, which mark its
//! heap: `--stress-incremental-marking` starts them from allocation, so
//! that V8 steps or finishes one at nearly every allocation of its. A
//! binding never lets V8 allocate in an operation of the DOM (its nodes
//! are no roots there); if one did, V8 would call the DOM's heap in it,
//! find the DOM borrowed and abort. Script builds trees, reads strings
//! (each new, so made after the operation), walks nodes whose wrappers
//! scavenges dropped (made after it, and wrapped in another), and lets
//! trees go; what it kept is whole after.
//!
//! Run locally: `cargo test -p dom_js --test marking`.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

use bindings::V8PerIsolateData;

const FLAGS: &str = "--expose-gc --stress-incremental-marking";

/// Whether this run's V8 marks incrementally: `MACHOPO_V8_FLAGS` (as
/// `test.py`'s fuzzer sets it) may turn it off, and then no marking of the
/// heap runs alongside script for these tests to see.
fn v8_marks_incrementally() -> bool {
    !std::env::var("MACHOPO_V8_FLAGS").is_ok_and(|flags| {
        flags
            .split_whitespace()
            .any(|flag| flag == "--no-incremental-marking")
    })
}

const SCRIPT: &str = r#"
const d = machopoDom.createDocument();
const root = d.createElement('root');
d.appendChild(root);
let kept = [];
for (let round = 0; round < 40; round++) {
    const list = d.createElement('ul');
    for (let i = 0; i < 300; i++) {
        const item = d.createElement('li');
        item.setAttribute('data-n', `${round}-${i}`);
        item.appendChild(d.createTextNode(`text ${i} é`));
        list.appendChild(item);
    }
    // Garbage on V8's heap: its allocation steps the markings.
    for (let i = 0; i < 200; i++) { const a = []; for (let j = 0; j < 50; j++) a.push({ j }); }
    if (round % 2 == 0) gc({ type: 'minor' });
    // Walks the list: wrappers the scavenge dropped are made again.
    let count = 0;
    for (let item = list.firstChild; item; item = item.nextSibling) {
        if (item.getAttribute('data-n') !== `${round}-${count}`) throw new Error('attribute');
        if (item.firstChild.data !== `text ${count} é`) throw new Error('text');
        count++;
    }
    if (count !== 300) throw new Error('count');
    // Every fourth list stays in the tree; the others go.
    if (round % 4 == 0) { root.appendChild(list); kept.push(round); }
}
for (let i = 0; i < 20; i++) { const a = []; for (let j = 0; j < 5000; j++) a.push({ j }); }
const lists = [];
for (let list = root.firstChild; list; list = list.nextSibling) lists.push(list.firstChild.getAttribute('data-n'));
lists.join()
"#;

#[test]
fn the_dom_lives_through_incremental_markings() {
    if !v8_marks_incrementally() {
        eprintln!("skipped: V8 doesn't mark incrementally in this run");
        return;
    }
    bindings::initialize_with_flags(FLAGS);
    let mut data = V8PerIsolateData::new();
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let global = context.global(scope);
    dom_js::install_machopo_dom(scope, global);
    let source = v8::String::new(scope, SCRIPT).unwrap();
    let result = {
        // Script entered, as the engine's entries count it.
        let _entry = bindings::trace::enter_script();
        v8::tc_scope!(let tc, scope);
        let result = v8::Script::compile(tc, source, None).and_then(|script| script.run(tc));
        match result {
            Some(value) => value.to_rust_string_lossy(tc),
            None => panic!("{}", tc.exception().unwrap().to_rust_string_lossy(tc)),
        }
    };
    let expected: Vec<String> = (0..40)
        .step_by(4)
        .map(|round| format!("{round}-0"))
        .collect();
    assert_eq!(result, expected.join(","));
    let stats = dom_js::heap_stats(scope).unwrap();
    assert!(
        stats.gcs > 2,
        "V8 marked the DOM's heap: {} times",
        stats.gcs
    );
}
