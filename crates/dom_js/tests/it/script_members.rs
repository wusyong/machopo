//! The members that run script, where what one call holds across its
//! script matters: an exception script threw is the very one the member
//! throws, a definition being made refuses another, every promise waiting
//! for a name resolves, each element's reactions keep their order, and no
//! microtask runs inside a member script called.

use crate::env::Env;

fn page() -> Env {
    let mut env = Env::window("about:blank");
    env.eval(
        "document.appendChild(document.createElement('html')).appendChild(document.createElement('body')); \
         function error(f) { try { f(); return 'no'; } catch (e) { return e.name; } } \
         function same(thrown, f) { try { f(); return 'no'; } catch (e) { return e === thrown; } } 0",
    );
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

/// What a getter `define` reads throws is what `define` throws: the same
/// object, not another of its name; and the registry takes definitions
/// again after.
#[test]
fn define_throws_the_very_exception_its_reads_threw() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var thrown = new RangeError('prototype'); \
                 var A = new Proxy(class {}, { get(target, name) { if (name === 'prototype') throw thrown; return target[name]; } }); \
                 same(thrown, () => customElements.define('s-a', A))",
                "true",
            ),
            (
                "thrown = { not: 'an error' }; class B { attributeChangedCallback() {} \
                   static get observedAttributes() { throw thrown; } }; \
                 same(thrown, () => customElements.define('s-b', B))",
                "true",
            ),
            (
                "thrown = new URIError('callback'); class C {}; \
                 Object.defineProperty(C.prototype, 'connectedCallback', { get() { throw thrown; } }); \
                 same(thrown, () => customElements.define('s-c', C))",
                "true",
            ),
            // None was defined, and the next is.
            (
                "[customElements.get('s-a'), customElements.get('s-b'), customElements.get('s-c')].join()",
                ",,",
            ),
            (
                "customElements.define('s-d', class extends HTMLElement {}); typeof customElements.get('s-d')",
                "function",
            ),
        ],
    );
}

/// A getter `define` reads that defines the same name is refused (a
/// definition is being made), and the outer one goes on to define it.
#[test]
fn define_refuses_another_of_the_same_name_from_its_reads() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var inner; class A extends HTMLElement {}; \
                 Object.defineProperty(A.prototype, 'connectedCallback', { get() { \
                   inner = error(() => customElements.define('n-a', class extends HTMLElement {})); return undefined; } }); \
                 customElements.define('n-a', A); [inner, customElements.get('n-a') === A].join()",
                "NotSupportedError,true",
            ),
            // The constructor being defined, under another name, too.
            (
                "class B extends HTMLElement {}; \
                 Object.defineProperty(B.prototype, 'disconnectedCallback', { get() { \
                   inner = error(() => customElements.define('n-c', B)); return undefined; } }); \
                 customElements.define('n-b', B); [inner, customElements.get('n-b') === B, customElements.get('n-c')].join()",
                "NotSupportedError,true,",
            ),
        ],
    );
}

/// Everything waiting for a name is told of its constructor as `define`
/// defines it: several reactions of the one promise, and each name's own.
#[test]
fn define_resolves_everything_waiting_for_the_name() {
    let mut env = page();
    env.eval(
        "var log = []; var first = customElements.whenDefined('w-a'); \
         first.then(c => log.push('1:' + (c === A))); first.then(c => log.push('2:' + (c === A))); \
         customElements.whenDefined('w-a').then(c => log.push('3:' + (c === A))); \
         customElements.whenDefined('w-b').then(() => log.push('other')); \
         class A extends HTMLElement {}; 0",
    );
    env.checkpoint();
    assert_eq!(env.eval("log.join()"), "", "nothing is defined yet");
    env.eval("customElements.define('w-a', A); log.push('defined'); 0");
    env.checkpoint();
    assert_eq!(env.eval("log.join()"), "defined,1:true,2:true,3:true");
    env.eval("customElements.define('w-b', class extends HTMLElement {}); 0");
    env.checkpoint();
    assert_eq!(env.eval("log.join()"), "defined,1:true,2:true,3:true,other");
}

/// One call's reactions run as it returns, each element's in the order
/// they were enqueued, the elements in tree order: an attribute's change
/// before the connection, of several elements made by one `innerHTML`.
#[test]
fn one_calls_reactions_run_in_order_across_its_elements() {
    let mut env = page();
    env.eval(
        "var log = []; class A extends HTMLElement { \
           static get observedAttributes() { return ['x', 'y']; } \
           constructor() { super(); log.push('new'); } \
           connectedCallback() { log.push('connected:' + this.id); } \
           attributeChangedCallback(name, old, value) { \
             log.push(name + '=' + value + ':' + this.id); if (hook) hook(this, name); } }; \
         var hook = null; customElements.define('o-a', A); 0",
    );
    check(
        &mut env,
        &[
            (
                "document.body.innerHTML = '<o-a id=1 x=a y=b></o-a><o-a id=2 y=c></o-a>'; log.push('returned'); log.join()",
                "new,x=a:1,y=b:1,connected:1,new,y=c:2,connected:2,returned",
            ),
            // A reaction that changes another element: that one's reaction
            // runs within it (its own queue), before the next of the call.
            (
                "log = []; var [one, two] = document.body.children; \
                 hook = (element, name) => { if (element === one && name === 'x') two.setAttribute('x', 'inner'); }; \
                 one.setAttribute('x', 'outer'); log.push('returned'); log.join()",
                "x=outer:1,x=inner:2,returned",
            ),
        ],
    );
}

/// A member script calls that runs script itself (a dispatch, a click, a
/// custom element's reactions, a traversal's filter) runs no microtask:
/// they wait for the script that called it to end.
#[test]
fn no_microtask_runs_inside_a_member_script_called() {
    let mut env = page();
    env.eval(
        "var log = []; var target = document.createElement('button'); document.body.appendChild(target); \
         var queue = name => Promise.resolve().then(() => log.push('microtask:' + name)); \
         target.addEventListener('x', () => { queue('x'); log.push('x'); }); \
         target.addEventListener('click', () => { queue('click'); log.push('click'); }); \
         customElements.define('m-a', class extends HTMLElement { connectedCallback() { queue('connected'); log.push('connected'); } }); \
         var walker = document.createTreeWalker(document.body, 1, () => { queue('filter'); log.push('filter'); return 1; }); \
         target.dispatchEvent(new Event('x')); log.push('after x'); \
         target.click(); log.push('after click'); \
         document.body.appendChild(document.createElement('m-a')); log.push('after append'); \
         walker.nextNode(); log.push('after walk'); 0",
    );
    env.checkpoint();
    assert_eq!(
        env.eval("log.join()"),
        "x,after x,click,after click,connected,after append,filter,after walk,\
         microtask:x,microtask:click,microtask:connected,microtask:filter"
    );
}

/// A mutation observer's callback that changes what it observes is called
/// again in the same checkpoint with those records; `takeRecords` in a
/// callback takes what the callback queued, which isn't delivered.
#[test]
fn a_mutation_callback_that_mutates_is_called_again() {
    let mut env = page();
    env.eval(
        "var log = []; var div = document.createElement('div'); var calls = 0; \
         var mo = new MutationObserver(records => { calls++; \
           log.push(calls + ':' + records.map(r => r.attributeName).join('+')); \
           if (calls === 1) div.setAttribute('b', '1'); \
           if (calls === 2) { div.setAttribute('c', '1'); log.push('took:' + mo.takeRecords().map(r => r.attributeName).join('+')); } }); \
         mo.observe(div, { attributes: true }); div.setAttribute('a', '1'); \
         Promise.resolve().then(() => log.push('promise')); 0",
    );
    env.checkpoint();
    assert_eq!(env.eval("log.join()"), "1:a,promise,2:b,took:c");
    assert_eq!(env.eval("calls"), "2");
}

/// What a traversal's filter throws is what the member throws, the same
/// object, of a function and of an object's `acceptNode`, a `TreeWalker`'s
/// and a `NodeIterator`'s; the traversal runs again after.
#[test]
fn a_traversal_throws_the_very_exception_its_filter_threw() {
    let mut env = page();
    env.eval("document.body.innerHTML = '<p></p><i></i>'; var thrown = { not: 'an error' }; var fail = true; 0");
    check(
        &mut env,
        &[
            (
                "var walker = document.createTreeWalker(document.body, 1, () => { if (fail) throw thrown; return 1; }); \
                 [same(thrown, () => walker.nextNode()), same(thrown, () => walker.firstChild())].join()",
                "true,true",
            ),
            (
                "var iterator = document.createNodeIterator(document.body, 1, { acceptNode() { if (fail) throw thrown; return 1; } }); \
                 same(thrown, () => iterator.nextNode())",
                "true",
            ),
            (
                "thrown = new RangeError('again'); same(thrown, () => iterator.nextNode())",
                "true",
            ),
            (
                "fail = false; [walker.nextNode().localName, iterator.nextNode().localName].join()",
                "p,body",
            ),
        ],
    );
}
