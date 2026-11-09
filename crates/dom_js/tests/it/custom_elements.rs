//! The custom element registry from script: `customElements` (the same
//! object each time), `define`'s checks and what it reads off its
//! constructor (in the steps' order, a getter's exception thrown as it
//! is), `get`, `getName`, and `whenDefined`'s promise (the same one until
//! the name is defined, resolved with the constructor then).

use crate::env::Env;

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

fn page() -> Env {
    let mut env = Env::window("about:blank");
    env.eval("function error(f) { try { f(); return 'no'; } catch (e) { return e.name; } } 0");
    env
}

#[test]
fn define_checks_its_arguments_in_order() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "[customElements === window.customElements, customElements instanceof CustomElementRegistry, \
                  Object.prototype.toString.call(customElements)].join()",
                "true,true,[object CustomElementRegistry]",
            ),
            ("error(() => customElements.define('a-b', {}))", "TypeError"),
            (
                "error(() => customElements.define('a-b', () => {}))",
                "TypeError",
            ),
            (
                "error(() => customElements.define('a-b', Math.max))",
                "TypeError",
            ),
            // The constructor is checked before the name.
            (
                "error(() => customElements.define('ab', () => {}))",
                "TypeError",
            ),
            (
                "error(() => customElements.define('ab', class {}))",
                "SyntaxError",
            ),
            (
                "error(() => customElements.define('A-b', class {}))",
                "SyntaxError",
            ),
            (
                "error(() => customElements.define('font-face', class {}))",
                "SyntaxError",
            ),
            (
                "error(() => customElements.define('a-b c', class {}))",
                "SyntaxError",
            ),
            (
                "error(() => customElements.define('a-\\u00b7', class {}))",
                "no",
            ),
            (
                "class A extends HTMLElement {}; customElements.define('a-a', A); customElements.get('a-a') === A",
                "true",
            ),
            (
                "error(() => customElements.define('a-a', class {}))",
                "NotSupportedError",
            ),
            (
                "error(() => customElements.define('a-c', A))",
                "NotSupportedError",
            ),
            (
                "error(() => customElements.define('a-d', class {}, { extends: 'p' }))",
                "NotSupportedError",
            ),
            // A name that is defined already is checked before `extends`.
            (
                "error(() => customElements.define('a-a', class {}, { extends: 'p' }))",
                "NotSupportedError",
            ),
            (
                "error(() => customElements.define('a-e', class {}, { extends: undefined }))",
                "no",
            ),
            (
                "error(() => customElements.define('a-f', class {}, null))",
                "no",
            ),
            ("error(() => customElements.define('a-g'))", "TypeError"),
        ],
    );
}

#[test]
fn define_reads_the_constructor_in_the_steps_order() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var log = []; \
                 var proto = new Proxy({ attributeChangedCallback() {} }, { get(t, k) { log.push(k); return t[k]; } }); \
                 var C = new Proxy(function () {}, { get(t, k) { log.push(k); return k === 'prototype' ? proto : t[k]; } }); \
                 customElements.define('r-a', C); log.join()",
                "prototype,connectedCallback,disconnectedCallback,adoptedCallback,attributeChangedCallback,\
                 observedAttributes,disabledFeatures,formAssociated",
            ),
            // `observedAttributes` only with an `attributeChangedCallback`.
            (
                "log = []; var plain = new Proxy({}, { get(t, k) { log.push(k); return t[k]; } }); \
                 var D = new Proxy(function () {}, { get(t, k) { log.push(k); return k === 'prototype' ? plain : t[k]; } }); \
                 customElements.define('r-b', D); log.join()",
                "prototype,connectedCallback,disconnectedCallback,adoptedCallback,attributeChangedCallback,\
                 disabledFeatures,formAssociated",
            ),
            // `formAssociated`: the form callbacks read, each checked.
            (
                "log = []; var E = new Proxy(function () {}, { get(t, k) { log.push(k); \
                   return k === 'prototype' ? plain : k === 'formAssociated' ? 1 : t[k]; } }); \
                 customElements.define('r-c', E); log.slice(6).join()",
                "formAssociated,formAssociatedCallback,formResetCallback,formDisabledCallback,formStateRestoreCallback",
            ),
            (
                "class F {}; F.prototype.formResetCallback = 1; F.formAssociated = true; \
                 [error(() => customElements.define('r-d', F)), customElements.get('r-d')].join()",
                "TypeError,",
            ),
        ],
    );
}

#[test]
fn define_throws_what_reading_the_constructor_throws() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "function G() {}; G.prototype = 1; error(() => customElements.define('t-a', G))",
                "TypeError",
            ),
            (
                "class H {}; H.prototype.connectedCallback = {}; error(() => customElements.define('t-b', H))",
                "TypeError",
            ),
            (
                "class I {}; Object.defineProperty(I.prototype, 'adoptedCallback', { get() { throw new RangeError('x'); } }); \
                 error(() => customElements.define('t-c', I))",
                "RangeError",
            ),
            (
                "class J { attributeChangedCallback() {} }; J.observedAttributes = 5; \
                 error(() => customElements.define('t-d', J))",
                "TypeError",
            ),
            (
                "class K {}; K.disabledFeatures = [{ toString() { throw new URIError(); } }]; \
                 error(() => customElements.define('t-e', K))",
                "URIError",
            ),
            // None of them was defined; the names stay free.
            (
                "['t-a', 't-b', 't-c', 't-d', 't-e'].map(name => customElements.get(name)).join() + '|' + \
                 error(() => customElements.define('t-c', class {}))",
                ",,,,|no",
            ),
            // `define` while one is reading its constructor.
            (
                "var inner; class L {}; Object.defineProperty(L.prototype, 'connectedCallback', \
                   { get() { inner = error(() => customElements.define('t-f', class {})); } }); \
                 customElements.define('t-g', L); [inner, customElements.get('t-f'), customElements.get('t-g') === L].join()",
                "NotSupportedError,,true",
            ),
            // Running is unset after a throw too.
            ("error(() => customElements.define('t-h', class {}))", "no"),
        ],
    );
}

#[test]
fn get_and_get_name_find_the_definition() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "class M {}; customElements.define('g-a', M); customElements.get('g-a') === M",
                "true",
            ),
            (
                "[customElements.get('g-b'), customElements.get('nothing-like-a-name-yet')].join()",
                ",",
            ),
            ("typeof customElements.get('G-A')", "undefined"),
            ("customElements.getName(M)", "g-a"),
            ("customElements.getName(class {})", "null"),
            ("error(() => customElements.getName(1))", "TypeError"),
            ("error(() => customElements.get())", "TypeError"),
        ],
    );
}

#[test]
fn when_defined_gives_one_promise_resolved_at_define() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var resolved = []; var p = customElements.whenDefined('w-a'); \
                 [p instanceof Promise, p === customElements.whenDefined('w-a'), \
                  p === customElements.whenDefined('w-b')].join()",
                "true,true,false",
            ),
            ("p.then(c => resolved.push(c.tag)); 0", "0"),
        ],
    );
    env.checkpoint();
    check(&mut env, &[("resolved.length", "0")]);
    // The constructor and the resolver live through GCs with no other
    // reference to them.
    env.eval("p = undefined; 0");
    env.full_gc();
    check(
        &mut env,
        &[(
            "class N {}; N.tag = 'n'; customElements.define('w-a', N); N = window.N = undefined; 0",
            "0",
        )],
    );
    env.full_gc();
    env.checkpoint();
    check(
        &mut env,
        &[
            ("customElements.get('w-a').tag", "n"),
            ("resolved.join()", "n"),
        ],
    );
    check(
        &mut env,
        &[
            (
                "var O = customElements.get('w-a'); var again = customElements.whenDefined('w-a'); \
                 again.then(c => resolved.push(c === O)); again === customElements.whenDefined('w-a')",
                "false",
            ),
            (
                "var rejected; customElements.whenDefined('nope').catch(e => { rejected = e.name; }); \
                 customElements.whenDefined('w-b') instanceof Promise",
                "true",
            ),
        ],
    );
    env.checkpoint();
    check(
        &mut env,
        &[(
            "[resolved.join(), rejected].join('|')",
            "n,true|SyntaxError",
        )],
    );
}

#[test]
fn a_definition_holds_its_constructor() {
    let mut env = page();
    // No promise, no job, no binding: the definition alone has it.
    check(
        &mut env,
        &[(
            "(() => { class Q {}; Q.tag = 'q'; customElements.define('h-q', Q); })(); 0",
            "0",
        )],
    );
    env.checkpoint();
    env.full_gc();
    env.full_gc();
    check(&mut env, &[("customElements.get('h-q').tag", "q")]);
}

#[test]
fn a_defined_constructor_makes_its_element() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "class X extends HTMLElement { constructor() { super(); this.made = (this.made || 0) + 1; } }; \
                 customElements.define('x-a', X); var x = new X(); \
                 [x instanceof X, x instanceof HTMLElement, x.localName, x.made, x.ownerDocument === document, \
                  x.parentNode, x.hasAttributes()].join()",
                "true,true,x-a,1,true,,false",
            ),
            (
                "var y = document.createElement('x-a'); [y instanceof X, y.made, y.localName, y !== x].join()",
                "true,1,x-a,true",
            ),
            // Custom already: not constructed again.
            (
                "customElements.upgrade(x); customElements.upgrade(y); [x.made, y.made].join()",
                "1,1",
            ),
            (
                "var z = document.createElementNS('http://www.w3.org/1999/xhtml', 'x-a'); [z instanceof X, z.made].join()",
                "true,1",
            ),
            // Not a defined constructor, or `HTMLElement` itself.
            ("error(() => new HTMLElement())", "TypeError"),
            (
                "class U extends HTMLElement {}; error(() => new U())",
                "TypeError",
            ),
            ("error(() => HTMLElement())", "TypeError"),
            // Defined with `HTMLElement` itself: still not constructed by it
            // (`new.target` is the active function).
            (
                "customElements.define('x-own', HTMLElement); \
                 [error(() => new HTMLElement()), error(() => Reflect.construct(HTMLElement, [], HTMLElement))].join()",
                "TypeError,TypeError",
            ),
            // `disabledFeatures`' `shadow`, looked up by its name.
            (
                "class N extends HTMLElement { static disabledFeatures = ['shadow']; }; customElements.define('x-n', N); \
                 [error(() => new N().attachShadow({ mode: 'open' })), \
                  error(() => document.createElement('x-n').attachShadow({ mode: 'open' })), \
                  error(() => new X().attachShadow({ mode: 'open' }))].join()",
                "NotSupportedError,NotSupportedError,no",
            ),
            // Another namespace's isn't constructed.
            (
                "document.createElementNS('http://www.w3.org/2000/svg', 'x-a') instanceof X",
                "false",
            ),
        ],
    );
}

#[test]
fn create_element_reports_a_constructor_that_fails() {
    let mut env = page();
    env.eval(
        "var errors = []; window.addEventListener('error', e => { errors.push(e.error.name); e.preventDefault(); }); 0",
    );
    check(
        &mut env,
        &[
            (
                "customElements.define('f-a', class extends HTMLElement { constructor() { super(); throw new RangeError(); } }); \
                 var a = document.createElement('f-a'); \
                 [a instanceof HTMLElement, a.localName, Object.getPrototypeOf(a) === HTMLUnknownElement.prototype, errors].join()",
                "true,f-a,true,RangeError",
            ),
            (
                "customElements.define('f-b', class extends HTMLElement { constructor() { super(); this.setAttribute('x', ''); } }); \
                 errors = []; var b = document.createElement('f-b'); [b.hasAttribute('x'), errors].join()",
                "false,NotSupportedError",
            ),
            (
                "customElements.define('f-c', class extends HTMLElement { constructor() { super(); this.append('t'); } }); \
                 errors = []; document.createElement('f-c'); errors.join()",
                "NotSupportedError",
            ),
            (
                "customElements.define('f-d', class extends HTMLElement { constructor() { super(); (document.documentElement || document.appendChild(document.createElement('html'))).append(this); } }); \
                 errors = []; document.createElement('f-d'); errors.join()",
                "NotSupportedError",
            ),
            (
                "customElements.define('f-g', class extends HTMLElement { constructor() { super(); \
                   document.implementation.createHTMLDocument('').adoptNode(this); } }); \
                 errors = []; document.createElement('f-g'); errors.join()",
                "NotSupportedError",
            ),
            (
                "customElements.define('f-h', class extends HTMLElement { constructor() { return new A(); } }); \
                 class A extends HTMLElement {}; customElements.define('f-i', A); \
                 errors = []; var h = document.createElement('f-h'); [errors, h.localName, h instanceof A].join()",
                "NotSupportedError,f-h,false",
            ),
            // Failed: not upgraded after.
            (
                "customElements.upgrade(a); a instanceof HTMLElement && Object.getPrototypeOf(a) === HTMLUnknownElement.prototype",
                "true",
            ),
            (
                "customElements.define('f-e', class { constructor() { return {}; } }); \
                 errors = []; document.createElement('f-e'); errors.join()",
                "TypeError",
            ),
            (
                "customElements.define('f-f', class extends HTMLElement { constructor() { return document.createElement('div'); } }); \
                 errors = []; document.createElement('f-f'); errors.join()",
                "NotSupportedError",
            ),
        ],
    );
}

#[test]
fn define_upgrades_the_connected_candidates_in_tree_order() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var body = document.documentElement || document.appendChild(document.createElement('html')); \
                 body.innerHTML = '<u-a id=one><u-a id=two></u-a></u-a><p></p>'; \
                 var host = body.appendChild(document.createElement('div')); \
                 host.attachShadow({ mode: 'open' }).innerHTML = '<u-a id=shadow></u-a>'; \
                 body.appendChild(document.createElement('u-a')).id = 'three'; \
                 var loose = document.createElement('u-a'); loose.id = 'loose'; \
                 var one = document.getElementById('one'); \
                 [one instanceof HTMLElement, one.constructor === HTMLElement].join()",
                "true,true",
            ),
            (
                "var order = []; class A extends HTMLElement { constructor() { super(); order.push(this.id); } }; \
                 customElements.define('u-a', A); order.join()",
                "one,two,shadow,three",
            ),
            (
                "[one instanceof A, loose instanceof A].join()",
                "true,false",
            ),
            // `upgrade` takes those out of the document too.
            (
                "customElements.upgrade(loose); [loose instanceof A, order.join()].join('|')",
                "true|one,two,shadow,three,loose",
            ),
            ("customElements.upgrade(loose); order.length", "5"),
            // Connected later: not upgraded here yet (an upgrade reaction).
            (
                "var later = document.createElement('u-b'); body.append(later); \
                 class B extends HTMLElement {}; body.removeChild(later); customElements.define('u-b', B); later instanceof B",
                "false",
            ),
        ],
    );
}

#[test]
fn an_element_upgraded_while_others_upgrade_is_constructed_once() {
    let mut env = page();
    check(
        &mut env,
        &[(
            "var box = (document.documentElement || document.appendChild(document.createElement('html'))) \
               .appendChild(document.createElement('div')); \
             box.innerHTML = '<n-a id=first></n-a><n-a id=second></n-a>'; var second = box.lastChild; \
             var made = []; class A extends HTMLElement { constructor() { super(); made.push(this.id); \
               if (this.id == 'first') customElements.upgrade(second); } }; \
             customElements.define('n-a', A); made.join()",
            "first,second",
        )],
    );
}

#[test]
fn an_upgrade_that_fails_leaves_the_element_failed() {
    let mut env = page();
    env.eval(
        "var errors = []; window.addEventListener('error', e => { errors.push(e.error.name); e.preventDefault(); }); \
         var html = (document.documentElement || document.appendChild(document.createElement('html'))) \
           .appendChild(document.createElement('div')); 0",
    );
    check(
        &mut env,
        &[
            (
                "html.innerHTML = '<e-a></e-a><e-b></e-b><e-c></e-c>'; \
                 customElements.define('e-a', class extends HTMLElement { constructor() { super(); throw new URIError(); } }); \
                 var a = html.firstChild; [errors, a.constructor === HTMLElement].join()",
                // Its prototype set by `super()`, before the throw.
                "URIError,false",
            ),
            // Failed: `upgrade` doesn't try it again.
            ("customElements.upgrade(a); errors.join()", "URIError"),
            (
                "errors = []; customElements.define('e-b', class { constructor() { return {}; } }); errors.join()",
                "TypeError",
            ),
            // One with a wrapper whose constructor gives another element.
            (
                "html.insertAdjacentHTML('beforeend', '<e-d></e-d>'); var d = html.lastChild; errors = []; \
                 customElements.define('e-d', class extends HTMLElement { constructor() { super(); \
                   return document.createElement('div'); } }); errors.join()",
                "TypeError",
            ),
            // A second construction while upgrading: the already
            // constructed marker.
            (
                "errors = []; var inner; class C extends HTMLElement { constructor() { super(); \
                   if (!inner) { inner = error(() => new C()); } } }; \
                 customElements.define('e-c', C); var c = html.querySelector('e-c'); [inner, c.localName, c instanceof C, errors.length].join()",
                "TypeError,e-c,true,0",
            ),
        ],
    );
}

#[test]
fn ce_reactions_upgrade_as_the_member_returns() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var html = document.documentElement || document.appendChild(document.createElement('html')); \
                 var early = document.createElement('r-a'); var made = []; \
                 class R extends HTMLElement { constructor() { super(); made.push(this.id || 'none'); } }; \
                 customElements.define('r-a', R); [made.length, early instanceof R].join()",
                "0,false",
            ),
            // Connected: upgraded as `appendChild` returns.
            (
                "early.id = 'early'; html.appendChild(early); [early instanceof R, made.join()].join('|')",
                "true|early",
            ),
            // A copy (not connected) and an import, as they return.
            (
                "made = []; var copy = early.cloneNode(); var imported = document.importNode(early); \
                 [copy instanceof R, imported instanceof R, copy !== early, made.join()].join('|')",
                "true|true|true|early,early",
            ),
            // The markup's, as `innerHTML` returns; not in a disconnected
            // container.
            (
                "made = []; html.insertAdjacentHTML('beforeend', '<div id=box><r-a id=m1></r-a><r-a id=m2></r-a></div>'); \
                 [made.join(), document.getElementById('m1') instanceof R].join('|')",
                "m1,m2|true",
            ),
            (
                "made = []; var loose = document.createElement('div'); loose.innerHTML = '<r-a id=l></r-a>'; \
                 [made.length, loose.firstChild instanceof R].join()",
                "0,false",
            ),
            (
                "html.append(loose); [made.join(), loose.firstChild instanceof R].join()",
                "l,true",
            ),
            // A reaction's script enqueues others: they run in their own
            // member's scope, before the outer one's next element.
            (
                "made = []; class S extends HTMLElement { constructor() { super(); made.push('s' + this.id); \
                   if (this.id == '1') html.append(Object.assign(document.createElement('r-a'), { id: 'inner' })); } }; \
                 var a = document.createElement('s-a'), b = document.createElement('s-a'); a.id = '1'; b.id = '2'; \
                 customElements.define('s-a', S); html.append(a, b); made.join()",
                // `r-a` constructed by `createElement`, before its id.
                "s1,none,s2",
            ),
        ],
    );
}

fn callbacks_page() -> Env {
    let mut env = page();
    env.eval(
        "var html = document.documentElement || document.appendChild(document.createElement('html')); \
         var log = []; \
         class L extends HTMLElement { \
           static observedAttributes = ['a', 'b']; \
           constructor() { super(); log.push('made ' + this.id); } \
           connectedCallback() { log.push('connected ' + this.id + ' ' + (this instanceof L)); } \
           disconnectedCallback() { log.push('disconnected ' + this.id); } \
           adoptedCallback(o, n) { log.push('adopted ' + this.id + ' ' + (o === document) + ' ' + (n !== document)); } \
           attributeChangedCallback(name, old, now, space) { log.push([this.id, name, old, now, space].join(':')); } \
         }; customElements.define('c-l', L); \
         function el(id) { var e = document.createElement('c-l'); e.id = id; return e; } 0",
    );
    env
}

#[test]
fn connected_and_disconnected_as_the_member_returns() {
    let mut env = callbacks_page();
    check(
        &mut env,
        &[
            (
                "log = []; var a = el('a'); a.append(el('b')); log.join('|')",
                "made |made ",
            ),
            (
                "log = []; html.append(a); log.join('|')",
                "connected a true|connected b true",
            ),
            (
                "log = []; html.removeChild(a); log.join('|')",
                "disconnected a|disconnected b",
            ),
            // Moved within the document: out, then in; each element's
            // reactions before the next element's.
            (
                "html.append(a); var box = html.appendChild(document.createElement('div')); log = []; box.append(a); log.join('|')",
                "disconnected a|connected a true|disconnected b|connected b true",
            ),
            // In a shadow tree, after its host.
            (
                "log = []; var host = el('h'); host.attachShadow({ mode: 'open' }).append(el('s')); log = []; box.append(host); log.join('|')",
                "connected h true|connected s true",
            ),
            // A member that fails after enqueuing: the reactions still run
            // before it throws.
            (
                "var o = el('o'); html.append(o); var spot = html.appendChild(document.createElement('i')); log = []; \
                 var thrown = error(() => spot.replaceWith(o, document)); [thrown, log.join('|'), o.isConnected].join('/')",
                "HierarchyRequestError/disconnected o/false",
            ),
            // Out of a document: none.
            (
                "log = []; var loose = document.createElement('div'); loose.append(el('x')); loose.firstChild.remove(); log.join('|')",
                "made ",
            ),
        ],
    );
}

#[test]
fn attribute_changed_for_observed_attributes() {
    let mut env = callbacks_page();
    check(
        &mut env,
        &[
            (
                "var e = el('e'); log = []; e.setAttribute('a', '1'); e.setAttribute('c', '3'); log.join('|')",
                "e:a::1:",
            ),
            (
                "log = []; e.setAttribute('a', '1'); e.setAttribute('a', '2'); log.join('|')",
                "e:a:1:1:|e:a:1:2:",
            ),
            (
                "log = []; e.removeAttribute('a'); e.removeAttribute('a'); log.join('|')",
                "e:a:2::",
            ),
            (
                "log = []; e.setAttributeNS('urn:x', 'p:b', 'v'); e.removeAttributeNS('urn:x', 'b'); log.join('|')",
                "e:b::v:urn:x|e:b:v::urn:x",
            ),
            // `id` isn't observed; a reflecting setter is a change too.
            ("log = []; e.id = 'f'; e.className = 'k'; log.join('|')", ""),
            (
                "log = []; e.toggleAttribute ? 0 : 0; e.dataset.x = 1; log.join('|')",
                "",
            ),
        ],
    );
}

/// The members that change attributes without naming one: an inline
/// style's (its attribute as it will be written, old the attribute as it
/// was) and the named and indexed setters' (`dataset`, an options
/// collection's), which call their callbacks as the member returns.
#[test]
fn attribute_changed_by_style_dataset_and_options() {
    let mut env = page();
    env.eval(
        "var html = document.documentElement || document.appendChild(document.createElement('html')); \
         var log = []; \
         class S extends HTMLElement { \
           static observedAttributes = ['style', 'data-x']; \
           disconnectedCallback() { log.push('disconnected'); } \
           attributeChangedCallback(name, old, now) { log.push([name, old, now].join(':')); } \
         }; customElements.define('s-s', S); 0",
    );
    check(
        &mut env,
        &[
            (
                "var s = document.createElement('s-s'); s.style.color = 'red'; log.join('|')",
                "style::color: red;",
            ),
            // `cssText` after a change not yet written to the attribute:
            // old is the attribute written from the declarations first.
            (
                "log = []; s.style.setProperty('width', '1px'); s.style.cssText = 'top: 0px'; log.join('|')",
                "style:color: red;:color: red; width: 1px;|style:color: red; width: 1px;:top: 0px;",
            ),
            (
                "log = []; s.style.removeProperty('top'); s.style.removeProperty('top'); log.join('|')",
                "style:top: 0px;:",
            ),
            ("log = []; s.dataset.x = 1; log.join('|')", "data-x::1"),
            (
                "var select = document.createElement('select'); html.append(select); \
                 var o = document.createElement('option'); o.append(document.createElement('s-s')); select.append(o); \
                 log = []; select.options[0] = new Option('b'); log.join('|')",
                "disconnected",
            ),
        ],
    );
}

/// An upgrade's and a synchronous creation's constructor is found by
/// `HTMLElement`'s constructor whichever realm's registry defines it (a
/// class of this window, defined in an iframe's registry), not only in
/// the current realm's; `new X()` of it from script still isn't. The
/// creation's element is the current realm's document's, so
/// `createElement` finds it of another document (a `NotSupportedError`,
/// not the `TypeError` of a constructor not found).
#[test]
fn a_constructor_defined_in_another_realms_registry() {
    let mut page = crate::history::Page::new("https://example.test/");
    page.load();
    assert_eq!(
        page.eval(
            "var errors = []; window.addEventListener('error', e => errors.push(e.error.name)); \
             var f = document.body.appendChild(document.createElement('iframe')); \
             var w = f.contentWindow, d = f.contentDocument; \
             var early = d.body.appendChild(d.createElement('x-a')); \
             var XA = class extends HTMLElement {}; w.customElements.define('x-a', XA); \
             var made = d.createElement('x-a'); \
             [early instanceof XA, made instanceof XA, errors.join()].join()"
        ),
        "true,false,NotSupportedError"
    );
    assert_eq!(
        page.eval("try { new XA(); 'made' } catch (e) { e.name }"),
        "TypeError"
    );
}

/// A definition that disables shadow roots doesn't upgrade an element
/// that has one: a `NotSupportedError` is reported in its constructor's
/// place, and the element stays as it was.
#[test]
fn an_upgrade_of_a_shadow_host_with_shadow_disabled_fails() {
    let mut env = page();
    check(
        &mut env,
        &[(
            "var error = null; window.addEventListener('error', e => { error = e.error; }); \
             class D extends HTMLElement { static disabledFeatures = ['shadow']; }; \
             var e = document.createElement('d-d'); e.attachShadow({ mode: 'open' }); \
             var f = document.createElement('d-d'); \
             customElements.define('d-d', D); customElements.upgrade(e); customElements.upgrade(f); \
             [e instanceof D, e.matches(':defined'), error && error.name, f instanceof D].join()",
            "false,false,NotSupportedError,true",
        )],
    );
}

#[test]
fn adopted_into_another_document() {
    let mut env = callbacks_page();
    check(
        &mut env,
        &[
            (
                "var other = document.implementation.createHTMLDocument(''); var d = el('d'); log = []; \
                 other.adoptNode(d); log.join('|')",
                "adopted d true true",
            ),
            // Back, connected: adopted, then connected.
            (
                "log = []; html.append(d); [log.join('|'), d.ownerDocument === document].join('/')",
                "adopted d false false|connected d true/true",
            ),
            // Into its own document again: no adoption.
            (
                "log = []; document.adoptNode(d); log.join('|')",
                "disconnected d",
            ),
        ],
    );
}

#[test]
fn an_upgrade_queues_its_attributes_and_connection() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var html = document.documentElement || document.appendChild(document.createElement('html')); \
                 html.insertAdjacentHTML('beforeend', '<u-p id=p a=1 z=2></u-p>'); var log = []; \
                 class U extends HTMLElement { static observedAttributes = ['a']; \
                   constructor() { super(); log.push('made'); } \
                   connectedCallback() { log.push('connected'); } \
                   attributeChangedCallback(n, o, v) { log.push(n + '=' + v + ' ' + o); } }; \
                 customElements.define('u-p', U); log.join('|')",
                "made|a=1 null|connected",
            ),
            // Custom after: connected again as it comes back.
            (
                "var p = document.getElementById('p'); log = []; p.remove(); html.append(p); log.join('|')",
                "connected",
            ),
            // One that throws: no callback after.
            (
                "html.insertAdjacentHTML('beforeend', '<u-q a=1></u-q>'); log = []; \
                 window.addEventListener('error', e => e.preventDefault()); \
                 class Q extends HTMLElement { static observedAttributes = ['a']; \
                   constructor() { super(); log.push('made'); throw 1; } \
                   connectedCallback() { log.push('connected'); } \
                   attributeChangedCallback() { log.push('changed'); } }; \
                 customElements.define('u-q', Q); log.join('|')",
                "made",
            ),
            // A callback's exception is reported, the others run.
            (
                "var errors = 0; window.addEventListener('error', () => errors++); log = []; \
                 class T extends HTMLElement { connectedCallback() { log.push(this.id); throw new Error(this.id); } }; \
                 customElements.define('u-t', T); var t1 = document.createElement('u-t'), t2 = document.createElement('u-t'); \
                 t1.id = 1; t2.id = 2; html.append(t1, t2); [log.join(), errors].join('|')",
                "1,2|2",
            ),
            // A definition with none of them: nothing.
            (
                "customElements.define('u-n', class extends HTMLElement {}); html.append(document.createElement('u-n')); 'ok'",
                "ok",
            ),
        ],
    );
}

#[test]
fn an_elements_child_nodes_leave_its_state_as_it_is() {
    let mut env = page();
    check(
        &mut env,
        &[
            // A node's `childNodes` and an element's custom element state
            // are kept side by side: neither changes the other.
            (
                "var d = document.createElement('div'); d.childNodes; d.matches(':defined')",
                "true",
            ),
            (
                "var u = document.createElement('x-u'); var list = u.childNodes; u.append('a'); [list.length, u.matches(':defined')].join()",
                "1,false",
            ),
        ],
    );
}
