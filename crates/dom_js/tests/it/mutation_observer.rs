//! `MutationObserver` from script: its records delivered in a microtask
//! (in order with promises), the callback's arguments and `this`, what
//! `innerHTML`, CSSOM and `attributeFilter` record.

use crate::env::Env;

#[test]
fn records_are_delivered_in_a_microtask() {
    let mut env = Env::window("about:blank");
    env.eval(
        "var log = []; var div = document.createElement('div'); \
         var mo = new MutationObserver(function (records, observer) { \
           log.push(['mo', this === mo, observer === mo, records.length, records[0].type, records[0] instanceof MutationRecord].join()); \
         }); \
         mo.observe(div, { attributes: true, childList: true }); \
         Promise.resolve().then(() => log.push('p1')); \
         div.setAttribute('a', '1'); div.setAttribute('a', '2'); \
         Promise.resolve().then(() => log.push('p2')); \
         log.push('sync');",
    );
    env.checkpoint();
    assert_eq!(
        env.eval("log.join(' | ')"),
        "sync | p1 | mo,true,true,2,attributes,true | p2"
    );
    let cases = [
        // Taken: none delivered.
        (
            "log = []; div.id = 'x'; var taken = mo.takeRecords(); [taken.length, taken[0].attributeName].join()",
            "1,id",
        ),
        (
            "div.innerHTML = '<p>a</p><p>b</p>'; var r = mo.takeRecords(); [r.length, r[0].addedNodes.length, r[0].target === div].join()",
            "1,2,true",
        ),
        (
            "div.innerHTML = ''; var r = mo.takeRecords(); [r[0].removedNodes.length, r[0].addedNodes.length].join()",
            "2,0",
        ),
        (
            "mo.disconnect(); div.id = 'y'; mo.takeRecords().length",
            "0",
        ),
        (
            "var o2 = new MutationObserver(() => {}); o2.observe(div, { attributeFilter: ['title'], attributeOldValue: true }); \
             div.setAttribute('title', 't1'); div.setAttribute('lang', 'en'); div.setAttribute('title', 't2'); \
             o2.takeRecords().map(r => r.attributeName + ':' + r.oldValue).join()",
            "title:null,title:t1",
        ),
        (
            "o2.observe(div, { attributes: true, attributeOldValue: true }); div.style.color = 'red'; div.style.color = 'blue'; \
             o2.takeRecords().map(r => r.attributeName + ':' + r.oldValue).join()",
            "style:null,style:color: red;",
        ),
        // Observed after a change not yet written to the attribute:
        // `cssText`'s old value is it written from the declarations.
        (
            "var d3 = document.createElement('div'); d3.style.color = 'red'; \
             o2.observe(d3, { attributes: true, attributeOldValue: true }); d3.style.cssText = 'top: 0px'; \
             o2.takeRecords().map(r => r.attributeName + ':' + r.oldValue).join()",
            "style:color: red;",
        ),
        (
            "try { new MutationObserver(1) } catch (e) { e instanceof TypeError }",
            "true",
        ),
        (
            "try { o2.observe(div, {}) } catch (e) { e instanceof TypeError }",
            "true",
        ),
        (
            "try { o2.observe(div, { childList: true, attributes: false, attributeFilter: ['a'] }) } catch (e) { e instanceof TypeError }",
            "true",
        ),
        (
            "try { MutationObserver(() => {}) } catch (e) { e instanceof TypeError }",
            "true",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
    // A callback that throws is reported; the next observer still runs.
    env.eval(
        "log = []; var a = new MutationObserver(() => { throw new Error('boom'); }); \
         var b = new MutationObserver(() => log.push('b')); \
         a.observe(div, { attributes: true }); b.observe(div, { attributes: true }); div.id = 'z';",
    );
    let before = env.uncaught_exceptions();
    env.checkpoint();
    assert_eq!(env.eval("log.join()"), "b");
    assert_eq!(env.uncaught_exceptions(), before + 1);
}

/// An observer script holds no more stays while a node it observes does
/// (its registration holds it), through V8's full GC.
#[test]
fn a_node_keeps_its_observer() {
    let mut env = Env::window("about:blank");
    env.eval(
        "var calls = 0; var node = document.createElement('div'); \
         (function () { new MutationObserver(() => { calls++; }).observe(node, { attributes: true }); })();",
    );
    env.full_gc();
    env.eval("node.id = 'x';");
    env.checkpoint();
    assert_eq!(env.eval("calls"), "1");
}
